//! `check_secrets` — repo-wide secret scanning via gitleaks. Reports only the
//! location (file/line) and rule of each leak, never the secret value. Graceful
//! when gitleaks is absent. A leak is data, not a tool error (is_error stays false).

use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};

use super::ToolResult;
use crate::runner::check_tool_available;
use crate::shell::safe_exec_in_dir;

const SCAN_TIMEOUT: Duration = Duration::from_secs(120);

struct SecretFinding {
    file: String,
    rule: String,
    line: Option<u64>,
}

/// Parse gitleaks JSON report into redacted findings (no secret values carried).
fn parse_gitleaks_json(json_str: &str) -> Vec<SecretFinding> {
    let parsed: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let arr = match parsed.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };
    arr.iter()
        .filter_map(|item| {
            let o = item.as_object()?;
            let file = o.get("File").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let rule = o
                .get("RuleID")
                .and_then(|v| v.as_str())
                .or_else(|| o.get("Description").and_then(|v| v.as_str()))
                .unwrap_or("secret")
                .to_string();
            let line = o.get("StartLine").and_then(|v| v.as_u64());
            Some(SecretFinding { file, rule, line })
        })
        .collect()
}

pub async fn execute_check_secrets(repo_path: &str) -> ToolResult {
    let repo = Path::new(repo_path);
    if !repo.is_dir() {
        return ToolResult {
            content: format!("repo_path is not a directory: {repo_path}"),
            is_error: true,
        };
    }

    if check_tool_available("gitleaks version").await.is_none() {
        return ToolResult {
            content: "gitleaks not installed — secret scan skipped. Install gitleaks \
                (https://github.com/gitleaks/gitleaks) to enable repo-wide secret detection."
                .to_string(),
            is_error: false,
        };
    }

    let report = std::env::temp_dir().join(format!("dt_gitleaks_{}.json", std::process::id()));
    let report_str = report.to_string_lossy().to_string();
    let args: Vec<String> = [
        "detect",
        "--no-git",
        "--report-format",
        "json",
        "--report-path",
        &report_str,
        "--source",
        ".",
        "--exit-code",
        "0",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    let _ = safe_exec_in_dir("gitleaks", &args, repo, SCAN_TIMEOUT).await;
    let json_str = std::fs::read_to_string(&report).unwrap_or_default();
    let _ = std::fs::remove_file(&report);

    let findings = parse_gitleaks_json(&json_str);

    let secrets_json: Vec<Value> = findings
        .iter()
        .map(|f| json!({ "file": f.file, "rule": f.rule, "line": f.line }))
        .collect();
    let body = json!({ "count": findings.len(), "secrets": secrets_json });

    let mut content = String::new();
    if findings.is_empty() {
        content.push_str("## Secret Scan\n\nNo secrets detected by gitleaks.\n\n");
    } else {
        content.push_str(&format!(
            "## Secret Scan\n\n{} potential secret(s) detected (locations only — values redacted):\n\n",
            findings.len()
        ));
        for f in &findings {
            let loc = f.line.map(|l| format!(":{l}")).unwrap_or_default();
            content.push_str(&format!("- `{}{}` — {}\n", f.file, loc, f.rule));
        }
        content.push('\n');
    }
    content.push_str(&format!(
        "```json\n{}\n```",
        serde_json::to_string_pretty(&body).unwrap_or_default()
    ));

    ToolResult {
        content,
        is_error: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[
      {"Description":"AWS Access Key","File":"src/config.py","StartLine":12,"RuleID":"aws-access-token","Secret":"AKIAIOSFODNN7EXAMPLE","Match":"key=AKIA..."},
      {"Description":"Generic API Key","File":"lib/client.ts","StartLine":3,"RuleID":"generic-api-key","Secret":"sk_live_abc123","Match":"token=sk_live..."}
    ]"#;

    #[test]
    fn parses_and_redacts() {
        let findings = parse_gitleaks_json(SAMPLE);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].file, "src/config.py");
        assert_eq!(findings[0].rule, "aws-access-token");
        assert_eq!(findings[0].line, Some(12));
        assert_eq!(findings[1].file, "lib/client.ts");
        // SecretFinding has no field for the secret value — redaction by construction.
    }

    #[test]
    fn handles_empty_and_invalid() {
        assert!(parse_gitleaks_json("[]").is_empty());
        assert!(parse_gitleaks_json("not json").is_empty());
        assert!(parse_gitleaks_json("{}").is_empty());
    }
}
