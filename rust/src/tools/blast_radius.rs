use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use ignore::WalkBuilder;
use regex::Regex;

use super::ToolResult;
use crate::lang::{language_for_path, SKIP_DIRS, SOURCE_EXTENSIONS};
use crate::shell::safe_exec;

const GIT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
struct ExtractedSymbol {
    name: String,
    kind: &'static str,
    file: String,
}

#[derive(Debug, Clone)]
struct DependentFile {
    path: String,
    depends_on: Vec<String>,
}

struct BlastRadiusResult {
    scope: String,
    changed_files: Vec<String>,
    symbols: Vec<ExtractedSymbol>,
    dependents: Vec<DependentFile>,
    errors: Vec<String>,
}

fn scope_to_diff_name_args(scope: &str) -> Vec<String> {
    match scope {
        "staged" => vec!["diff".into(), "--cached".into(), "--name-only".into()],
        "unpushed" => vec![
            "log".into(),
            "--name-only".into(),
            "--pretty=format:".into(),
            "origin/HEAD..HEAD".into(),
        ],
        range => vec!["diff".into(), "--name-only".into(), range.into()],
    }
}

fn scope_to_diff_args(scope: &str) -> Vec<String> {
    match scope {
        "staged" => vec!["diff".into(), "--cached".into(), "-U0".into()],
        "unpushed" => vec!["diff".into(), "origin/HEAD..HEAD".into(), "-U0".into()],
        range => vec!["diff".into(), range.into(), "-U0".into()],
    }
}

fn extract_symbols_from_diff(diff: &str, file_path: &str) -> Vec<ExtractedSymbol> {
    let lang = match language_for_path(file_path) {
        Some(l) => l,
        None => return Vec::new(),
    };

    let patterns = symbol_patterns(lang);
    let mut symbols = Vec::new();
    let mut seen = HashSet::new();

    for line in diff.lines() {
        if !line.starts_with('+') || line.starts_with("+++") {
            continue;
        }
        let content = &line[1..];
        for (kind, re) in &patterns {
            for cap in re.captures_iter(content) {
                if let Some(name) = cap.get(1) {
                    let name_str = name.as_str().to_string();
                    if seen.insert(name_str.clone()) {
                        symbols.push(ExtractedSymbol {
                            name: name_str,
                            kind,
                            file: file_path.to_string(),
                        });
                    }
                }
            }
        }
    }

    symbols
}

fn symbol_patterns(lang: &str) -> Vec<(&'static str, Regex)> {
    match lang {
        "rust" => vec![
            ("function", Regex::new(r"pub\s+(?:async\s+)?fn\s+(\w+)").unwrap()),
            ("struct", Regex::new(r"pub\s+struct\s+(\w+)").unwrap()),
            ("enum", Regex::new(r"pub\s+enum\s+(\w+)").unwrap()),
            ("trait", Regex::new(r"pub\s+trait\s+(\w+)").unwrap()),
            ("type", Regex::new(r"pub\s+type\s+(\w+)").unwrap()),
        ],
        "typescript" | "javascript" => vec![
            ("function", Regex::new(r"export\s+(?:async\s+)?function\s+(\w+)").unwrap()),
            ("class", Regex::new(r"export\s+class\s+(\w+)").unwrap()),
            ("const", Regex::new(r"export\s+(?:const|let|var)\s+(\w+)").unwrap()),
            ("interface", Regex::new(r"export\s+interface\s+(\w+)").unwrap()),
            ("type", Regex::new(r"export\s+type\s+(\w+)").unwrap()),
            ("enum", Regex::new(r"export\s+enum\s+(\w+)").unwrap()),
        ],
        "python" => vec![
            ("function", Regex::new(r"^def\s+(\w+)").unwrap()),
            ("class", Regex::new(r"^class\s+(\w+)").unwrap()),
        ],
        "go" => vec![
            ("function", Regex::new(r"^func\s+(?:\([^)]*\)\s+)?([A-Z]\w*)").unwrap()),
            ("type", Regex::new(r"^type\s+([A-Z]\w*)").unwrap()),
        ],
        "java" => vec![
            ("class", Regex::new(r"public\s+(?:abstract\s+)?class\s+(\w+)").unwrap()),
            ("interface", Regex::new(r"public\s+interface\s+(\w+)").unwrap()),
            ("method", Regex::new(r"public\s+(?:static\s+)?(?:\w+\s+)+(\w+)\s*\(").unwrap()),
        ],
        "php" => vec![
            ("function", Regex::new(r"(?:public|protected)\s+function\s+(\w+)").unwrap()),
            ("class", Regex::new(r"class\s+(\w+)").unwrap()),
        ],
        "csharp" => vec![
            ("class", Regex::new(r"public\s+(?:partial\s+)?class\s+(\w+)").unwrap()),
            ("method", Regex::new(r"public\s+(?:static\s+)?(?:async\s+)?\w+\s+(\w+)\s*\(").unwrap()),
            ("interface", Regex::new(r"public\s+interface\s+(\w+)").unwrap()),
        ],
        _ => Vec::new(),
    }
}

fn scan_file_for_references(content: &str, symbols: &[ExtractedSymbol], compiled_regexes: &[Regex]) -> Vec<String> {
    let mut found = Vec::new();
    for (sym, re) in symbols.iter().zip(compiled_regexes.iter()) {
        if re.is_match(content) {
            found.push(sym.name.clone());
        }
    }
    found
}

fn format_blast_radius(result: &BlastRadiusResult) -> String {
    let mut lines = vec![
        "# Blast Radius Analysis".to_string(),
        String::new(),
        format!("**Scope:** {}", result.scope),
        format!("**Changed files:** {}", result.changed_files.len()),
        format!("**Affected symbols:** {}", result.symbols.len()),
        format!("**Dependent files:** {}", result.dependents.len()),
    ];

    if result.changed_files.is_empty() {
        lines.push(String::new());
        lines.push("No changed files found for this scope.".to_string());
        return lines.join("\n");
    }

    lines.push(String::new());
    lines.push("## Changed Files".to_string());
    for f in &result.changed_files {
        lines.push(format!("- `{f}`"));
    }

    if !result.symbols.is_empty() {
        lines.push(String::new());
        lines.push("## Changed Symbols".to_string());
        lines.push("| Symbol | Kind | File |".to_string());
        lines.push("|--------|------|------|".to_string());
        for s in &result.symbols {
            lines.push(format!("| `{}` | {} | `{}` |", s.name, s.kind, s.file));
        }
    }

    if !result.dependents.is_empty() {
        lines.push(String::new());
        lines.push("## Dependent Files".to_string());
        for d in &result.dependents {
            let deps = d.depends_on.join("`, `");
            lines.push(format!("- `{}` — uses: `{deps}`", d.path));
        }
    }

    if !result.errors.is_empty() {
        lines.push(String::new());
        lines.push("## Warnings".to_string());
        for e in &result.errors {
            lines.push(format!("- {e}"));
        }
    }

    lines.join("\n")
}

pub async fn execute_blast_radius(repo_path: &str, scope: &str) -> ToolResult {
    let repo = Path::new(repo_path);
    if !repo.join(".git").exists() {
        return ToolResult {
            content: format!("Not a git repository: {repo_path}"),
            is_error: true,
        };
    }

    let mut errors = Vec::new();

    // Get changed file list
    let name_args = scope_to_diff_name_args(scope);
    let result = safe_exec("git", &[
        vec!["-C".to_string(), repo_path.to_string()],
        name_args,
    ].concat(), GIT_TIMEOUT).await;

    if result.exit_code != 0 {
        // Try fallback for unpushed: origin/main..HEAD
        if scope == "unpushed" {
            let fallback = safe_exec("git", &[
                "-C".to_string(), repo_path.to_string(),
                "log".to_string(), "--name-only".to_string(),
                "--pretty=format:".to_string(), "origin/main..HEAD".to_string(),
            ], GIT_TIMEOUT).await;
            if fallback.exit_code != 0 {
                return ToolResult {
                    content: format!("git diff failed: {}. Ensure the scope is valid and remote is set.", result.stderr.trim()),
                    is_error: true,
                };
            }
            // Use fallback result below
            let changed_files: Vec<String> = fallback.stdout.lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
                .collect();
            return execute_with_files(repo_path, scope, &changed_files, "origin/main..HEAD", &mut errors).await;
        }
        return ToolResult {
            content: format!("git diff failed: {}", result.stderr.trim()),
            is_error: true,
        };
    }

    let changed_files: Vec<String> = result.stdout.lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();

    let diff_ref = match scope {
        "staged" => "staged",
        "unpushed" => "origin/HEAD..HEAD",
        other => other,
    };

    execute_with_files(repo_path, scope, &changed_files, diff_ref, &mut errors).await
}

async fn execute_with_files(
    repo_path: &str,
    scope: &str,
    changed_files: &[String],
    diff_ref: &str,
    errors: &mut Vec<String>,
) -> ToolResult {
    if changed_files.is_empty() {
        let result = BlastRadiusResult {
            scope: scope.to_string(),
            changed_files: Vec::new(),
            symbols: Vec::new(),
            dependents: Vec::new(),
            errors: Vec::new(),
        };
        return ToolResult {
            content: format_blast_radius(&result),
            is_error: false,
        };
    }

    // Get full diff for symbol extraction
    let diff_args = scope_to_diff_args(diff_ref);
    let diff_result = safe_exec("git", &[
        vec!["-C".to_string(), repo_path.to_string()],
        diff_args,
    ].concat(), GIT_TIMEOUT).await;

    let mut all_symbols = Vec::new();

    if diff_result.exit_code == 0 {
        // Split diff by file
        let mut current_file = String::new();
        let mut current_chunk = String::new();

        for line in diff_result.stdout.lines() {
            if line.starts_with("diff --git") {
                if !current_file.is_empty() && !current_chunk.is_empty() {
                    all_symbols.extend(extract_symbols_from_diff(&current_chunk, &current_file));
                }
                current_chunk.clear();
                // Extract filename from "diff --git a/path b/path"
                if let Some(b_path) = line.split(" b/").last() {
                    current_file = b_path.to_string();
                }
            } else {
                current_chunk.push_str(line);
                current_chunk.push('\n');
            }
        }
        if !current_file.is_empty() && !current_chunk.is_empty() {
            all_symbols.extend(extract_symbols_from_diff(&current_chunk, &current_file));
        }
    } else {
        errors.push(format!("Could not get full diff: {}", diff_result.stderr.trim()));
    }

    // Scan repo for dependents
    let mut dependents = Vec::new();
    let changed_set: HashSet<&str> = changed_files.iter().map(|s| s.as_str()).collect();

    if !all_symbols.is_empty() {
        // Pre-compile all symbol regexes once before scanning files
        let compiled_regexes: Vec<Regex> = all_symbols
            .iter()
            .filter_map(|sym| {
                let pattern = format!(r"\b{}\b", regex::escape(&sym.name));
                Regex::new(&pattern).ok()
            })
            .collect();

        // If any regex failed to compile, filter symbols to match
        let (symbols_to_use, regexes_to_use) = if compiled_regexes.len() == all_symbols.len() {
            (&all_symbols[..], &compiled_regexes[..])
        } else {
            // Rebuild with only successfully compiled pairs
            // This shouldn't happen in practice since symbol names are word chars
            errors.push("Some symbol regexes failed to compile".to_string());
            (&all_symbols[..compiled_regexes.len()], &compiled_regexes[..])
        };

        let repo_path_obj = Path::new(repo_path);
        let walker = WalkBuilder::new(repo_path)
            .hidden(true)
            .git_ignore(true)
            .filter_entry(|entry| {
                if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                    let name = entry.file_name().to_string_lossy();
                    return !SKIP_DIRS.contains(&name.as_ref());
                }
                true
            })
            .build();

        for entry in walker.flatten() {
            if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                continue;
            }
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !SOURCE_EXTENSIONS.contains(&ext) {
                continue;
            }

            let rel_path = path.strip_prefix(repo_path_obj)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            if changed_set.contains(rel_path.as_str()) {
                continue;
            }

            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let found = scan_file_for_references(&content, symbols_to_use, regexes_to_use);
            if !found.is_empty() {
                dependents.push(DependentFile {
                    path: rel_path,
                    depends_on: found,
                });
            }
        }
    }

    dependents.sort_by(|a, b| a.path.cmp(&b.path));

    let result = BlastRadiusResult {
        scope: scope.to_string(),
        changed_files: changed_files.to_vec(),
        symbols: all_symbols,
        dependents,
        errors: errors.clone(),
    };

    ToolResult {
        content: format_blast_radius(&result),
        is_error: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_to_diff_name_args() {
        let args = scope_to_diff_name_args("staged");
        assert_eq!(args, vec!["diff", "--cached", "--name-only"]);

        let args = scope_to_diff_name_args("unpushed");
        assert!(args.contains(&"origin/HEAD..HEAD".to_string()));

        let args = scope_to_diff_name_args("main..HEAD");
        assert!(args.contains(&"main..HEAD".to_string()));
    }

    #[test]
    fn test_extract_symbols_rust() {
        let diff = r#"+pub fn execute_review(agent: &AgentDefinition) -> ToolResult {
+pub struct ToolResult {
+    pub content: String,
+pub enum AgentRole {
+pub trait Reviewable {
"#;
        let symbols = extract_symbols_from_diff(diff, "src/review.rs");
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"execute_review"));
        assert!(names.contains(&"ToolResult"));
        assert!(names.contains(&"AgentRole"));
        assert!(names.contains(&"Reviewable"));
    }

    #[test]
    fn test_extract_symbols_typescript() {
        let diff = r#"+export function handleRequest(req: Request) {
+export class AuthService {
+export const MAX_RETRIES = 3;
+export interface Config {
+export type Handler = () => void;
"#;
        let symbols = extract_symbols_from_diff(diff, "src/handler.ts");
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"handleRequest"));
        assert!(names.contains(&"AuthService"));
        assert!(names.contains(&"MAX_RETRIES"));
        assert!(names.contains(&"Config"));
        assert!(names.contains(&"Handler"));
    }

    #[test]
    fn test_extract_symbols_python() {
        let diff = "+def process_data(items):\n+class DataProcessor:\n";
        let symbols = extract_symbols_from_diff(diff, "app/processor.py");
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"process_data"));
        assert!(names.contains(&"DataProcessor"));
    }

    #[test]
    fn test_extract_symbols_go() {
        let diff = "+func HandleRequest(w http.ResponseWriter, r *http.Request) {\n+type Config struct {\n";
        let symbols = extract_symbols_from_diff(diff, "main.go");
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"HandleRequest"));
        assert!(names.contains(&"Config"));
    }

    #[test]
    fn test_scan_references() {
        let content = "use crate::types::AgentDefinition;\nlet agent: AgentDefinition = todo!();";
        let symbols = vec![
            ExtractedSymbol { name: "AgentDefinition".into(), kind: "struct", file: "types.rs".into() },
            ExtractedSymbol { name: "UnusedThing".into(), kind: "struct", file: "types.rs".into() },
        ];
        let compiled_regexes: Vec<Regex> = symbols
            .iter()
            .map(|sym| Regex::new(&format!(r"\b{}\b", regex::escape(&sym.name))).unwrap())
            .collect();
        let found = scan_file_for_references(content, &symbols, &compiled_regexes);
        assert!(found.contains(&"AgentDefinition".to_string()));
        assert!(!found.contains(&"UnusedThing".to_string()));
    }
}
