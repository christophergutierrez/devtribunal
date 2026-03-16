use std::time::Duration;

use serde_json::Value;

use crate::runner::expand_runner_alternatives;
use crate::shell::{safe_exec, split_command, validate_file_path};
use crate::types::{LinterFinding, LinterRunResult, RecommendedTool};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// Check if a tool is installed, trying package-runner alternatives.
/// Returns the runner that works, or None.
async fn check_installed(tool: &RecommendedTool) -> Option<String> {
    if tool.check.is_empty() {
        return None;
    }

    let alternatives = expand_runner_alternatives(&tool.check);
    for alt in &alternatives {
        let (bin, args) = split_command(&alt.cmd);
        let result = safe_exec(&bin, &args, CHECK_TIMEOUT).await;
        if result.exit_code == 0 {
            return Some(alt.runner.clone());
        }
    }
    None
}

/// Strip shell redirections like 2>&1 from command arguments.
fn strip_redirections(args: &[String]) -> (Vec<String>, bool) {
    let mut merge_stderr = false;
    let clean: Vec<String> = args
        .iter()
        .filter(|a| {
            if a.as_str() == "2>&1" {
                merge_stderr = true;
                false
            } else {
                true
            }
        })
        .cloned()
        .collect();
    (clean, merge_stderr)
}

/// Try to parse JSON linter output into findings.
fn try_parse_json_findings(tool: &RecommendedTool, stdout: &str) -> Vec<LinterFinding> {
    let parsed: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(_) => {
            // JSON parse failed — return raw output as single finding
            let trimmed = stdout.trim();
            if trimmed.is_empty() {
                return Vec::new();
            }
            return vec![LinterFinding {
                tool: tool.name.clone(),
                file: String::new(),
                line: None,
                column: None,
                severity: "info".to_string(),
                message: trimmed.to_string(),
                rule: None,
            }];
        }
    };

    let items = extract_items(&parsed);
    items
        .iter()
        .filter_map(|item| {
            let obj = item.as_object()?;
            Some(LinterFinding {
                tool: tool.name.clone(),
                file: get_str(obj, &["file", "path", "filename", "filePath"]),
                line: get_number(obj, &["line", "row", "startLine", "begin_line"]),
                column: get_number(obj, &["column", "col", "startColumn", "begin_column"]),
                severity: get_str(obj, &["severity", "type", "level"])
                    .to_lowercase()
                    .replace(|c: char| c.is_whitespace(), ""),
                message: get_str(obj, &["message", "msg", "description"]),
                rule: extract_rule_id(obj),
            })
        })
        .collect()
}

/// Extract a string from an object, trying multiple field names.
fn get_str(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> String {
    for key in keys {
        if let Some(val) = obj.get(*key) {
            if let Some(s) = val.as_str() {
                return s.to_string();
            }
            if !val.is_null() {
                return val.to_string();
            }
        }
    }
    String::new()
}

/// Extract a number from an object, trying multiple field names.
fn get_number(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<u32> {
    for key in keys {
        if let Some(val) = obj.get(*key) {
            if let Some(n) = val.as_u64() {
                return Some(n as u32);
            }
            if let Some(s) = val.as_str() {
                if let Ok(n) = s.parse::<u32>() {
                    return Some(n);
                }
            }
        }
    }
    None
}

/// Extract a rule ID from various linter JSON shapes.
fn extract_rule_id(obj: &serde_json::Map<String, Value>) -> Option<String> {
    // rule.id (nested object)
    if let Some(rule) = obj.get("rule") {
        if let Some(rule_obj) = rule.as_object() {
            if let Some(id) = rule_obj.get("id").and_then(|v| v.as_str()) {
                return Some(id.to_string());
            }
        }
        if let Some(s) = rule.as_str() {
            return Some(s.to_string());
        }
    }
    // ruleId
    if let Some(v) = obj.get("ruleId").and_then(|v| v.as_str()) {
        return Some(v.to_string());
    }
    // code
    if let Some(v) = obj.get("code") {
        if let Some(s) = v.as_str() {
            return Some(s.to_string());
        }
        if let Some(n) = v.as_u64() {
            return Some(n.to_string());
        }
    }
    None
}

/// Extract an array of finding items from various JSON structures.
fn extract_items(parsed: &Value) -> Vec<&Value> {
    // Direct array: [{...}, {...}]
    if let Some(arr) = parsed.as_array() {
        return arr.iter().collect();
    }

    if let Some(obj) = parsed.as_object() {
        // eslint: { results: [{ filePath, messages: [...] }] }
        if let Some(results) = obj.get("results").and_then(|v| v.as_array()) {
            let mut items = Vec::new();
            for result in results {
                if let Some(r) = result.as_object() {
                    if let Some(messages) = r.get("messages").and_then(|v| v.as_array()) {
                        items.extend(messages.iter());
                    }
                }
            }
            if !items.is_empty() {
                return items;
            }
        }

        // biome: { diagnostics: [...] }
        if let Some(arr) = obj.get("diagnostics").and_then(|v| v.as_array()) {
            return arr.iter().collect();
        }

        // phpstan: { errors: [...] }
        if let Some(arr) = obj.get("errors").and_then(|v| v.as_array()) {
            return arr.iter().collect();
        }

        // golangci-lint: { issues: [...] }
        if let Some(arr) = obj.get("issues").and_then(|v| v.as_array()) {
            return arr.iter().collect();
        }
    }

    Vec::new()
}

fn parse_text_output(tool: &RecommendedTool, stdout: &str, stderr: &str) -> Vec<LinterFinding> {
    let output = format!("{stdout}\n{stderr}").trim().to_string();
    if output.is_empty() {
        return Vec::new();
    }
    vec![LinterFinding {
        tool: tool.name.clone(),
        file: String::new(),
        line: None,
        column: None,
        severity: "info".to_string(),
        message: output,
        rule: None,
    }]
}

fn parse_linter_output(
    tool: &RecommendedTool,
    stdout: &str,
    stderr: &str,
) -> Vec<LinterFinding> {
    if tool.output_format.is_empty() {
        return Vec::new();
    }
    match tool.output_format.as_str() {
        "json" => try_parse_json_findings(tool, stdout),
        "text" => parse_text_output(tool, stdout, stderr),
        _ => parse_text_output(tool, stdout, stderr),
    }
}

/// Format linter findings into Markdown for inclusion in the review prompt.
pub fn format_linter_findings(result: &LinterRunResult) -> String {
    if result.findings.is_empty() && result.skipped.is_empty() && result.errors.is_empty() {
        return String::new();
    }

    let mut lines = vec!["\n## Linter Findings\n".to_string()];

    if !result.findings.is_empty() {
        for f in &result.findings {
            let location = if !f.file.is_empty() {
                if let Some(line) = f.line {
                    format!("{}:{} — ", f.file, line)
                } else {
                    format!("{} — ", f.file)
                }
            } else {
                String::new()
            };
            lines.push(format!(
                "**{}** [{}] {}{}", f.tool, f.severity, location, f.message
            ));
        }
    } else {
        lines.push("No issues found by linters.".to_string());
    }

    if !result.skipped.is_empty() {
        lines.push(String::new());
        lines.push(format!("**Not installed (skipped):** {}", result.skipped.join(", ")));
    }

    if !result.errors.is_empty() {
        lines.push(String::new());
        lines.push(format!("**Errors:** {}", result.errors.join("; ")));
    }

    lines.join("\n")
}

/// Run all recommended linters for a file, returning aggregated results.
pub async fn run_linters(
    file_path: &str,
    tools: &[RecommendedTool],
) -> anyhow::Result<LinterRunResult> {
    validate_file_path(file_path).map_err(|e| anyhow::anyhow!(e))?;

    let mut skipped = Vec::new();

    // Filter to tools with run commands
    let runnable: Vec<&RecommendedTool> = tools.iter().filter(|t| !t.run.is_empty()).collect();

    // First pass: check installation, build execution list
    struct ToRun {
        tool: RecommendedTool,
        bin: String,
        args: Vec<String>,
        merge_stderr: bool,
    }

    let mut to_run = Vec::new();

    for tool in &runnable {
        let runner = match check_installed(tool).await {
            Some(r) => r,
            None => {
                skipped.push(tool.name.clone());
                continue;
            }
        };

        let mut cmd_template = tool.run.clone();
        if runner != "system" {
            let alts = expand_runner_alternatives(&cmd_template);
            if let Some(matched) = alts.iter().find(|a| a.runner == runner) {
                cmd_template = matched.cmd.clone();
            }
        }

        let expanded = cmd_template.replace("{file}", file_path);
        let (bin, raw_args) = split_command(&expanded);
        let (clean_args, merge_stderr) = strip_redirections(&raw_args);

        to_run.push(ToRun {
            tool: (*tool).clone(),
            bin,
            args: clean_args,
            merge_stderr,
        });
    }

    // Second pass: execute all in parallel
    let handles: Vec<_> = to_run
        .into_iter()
        .map(|tr| {
            tokio::spawn(async move {
                let result = safe_exec(&tr.bin, &tr.args, DEFAULT_TIMEOUT).await;
                if result.exit_code == -1 {
                    return (tr.tool, Some(result.stderr), Vec::new());
                }
                let stdout = if tr.merge_stderr {
                    format!("{}\n{}", result.stdout, result.stderr)
                } else {
                    result.stdout
                };
                let findings = parse_linter_output(&tr.tool, &stdout, &result.stderr);
                (tr.tool, None, findings)
            })
        })
        .collect();

    let mut findings = Vec::new();
    let mut errors = Vec::new();

    for handle in handles {
        match handle.await {
            Ok((_, Some(err), _)) => errors.push(err),
            Ok((_, None, f)) => findings.extend(f),
            Err(e) => errors.push(format!("Task failed: {e}")),
        }
    }

    Ok(LinterRunResult {
        findings,
        skipped,
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool(name: &str) -> RecommendedTool {
        RecommendedTool {
            name: name.to_string(),
            check: String::new(),
            run: String::new(),
            output_format: "json".to_string(),
            purpose: String::new(),
        }
    }

    #[test]
    fn test_parse_eslint_json() {
        let tool = make_tool("eslint");
        let json = r#"{
            "results": [{
                "filePath": "/tmp/test.ts",
                "messages": [
                    {"line": 10, "column": 5, "severity": 2, "message": "no-unused-vars", "ruleId": "no-unused-vars"}
                ]
            }]
        }"#;
        let findings = try_parse_json_findings(&tool, json);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].message, "no-unused-vars");
        assert_eq!(findings[0].line, Some(10));
        assert_eq!(findings[0].rule.as_deref(), Some("no-unused-vars"));
    }

    #[test]
    fn test_parse_direct_array() {
        let tool = make_tool("ruff");
        let json = r#"[
            {"file": "test.py", "line": 5, "severity": "warning", "message": "unused import", "code": "F401"}
        ]"#;
        let findings = try_parse_json_findings(&tool, json);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].file, "test.py");
        assert_eq!(findings[0].rule.as_deref(), Some("F401"));
    }

    #[test]
    fn test_parse_golangci_issues() {
        let tool = make_tool("golangci-lint");
        let json = r#"{"issues": [{"file": "main.go", "line": 42, "severity": "error", "message": "ineffective assignment"}]}"#;
        let findings = try_parse_json_findings(&tool, json);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, Some(42));
    }

    #[test]
    fn test_parse_invalid_json() {
        let tool = make_tool("test");
        let findings = try_parse_json_findings(&tool, "not json at all");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, "info");
        assert!(findings[0].message.contains("not json"));
    }

    #[test]
    fn test_format_empty() {
        let result = LinterRunResult {
            findings: Vec::new(),
            skipped: Vec::new(),
            errors: Vec::new(),
        };
        assert!(format_linter_findings(&result).is_empty());
    }

    #[test]
    fn test_strip_redirections() {
        let args = vec!["--flag".to_string(), "2>&1".to_string(), "file.ts".to_string()];
        let (clean, merge) = strip_redirections(&args);
        assert_eq!(clean, vec!["--flag", "file.ts"]);
        assert!(merge);
    }
}
