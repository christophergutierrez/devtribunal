use std::collections::HashSet;
use std::path::Path;

use crate::tools::review::ToolResult;
use crate::types::{embedded_skills, parse_agent};

const EXTENSION_TO_LANGUAGE: &[(&str, &str)] = &[
    (".ts", "typescript"),
    (".tsx", "typescript"),
    (".js", "javascript"),
    (".jsx", "javascript"),
    (".py", "python"),
    (".rs", "rust"),
    (".go", "go"),
    (".java", "java"),
    (".php", "php"),
    (".cs", "csharp"),
    (".c", "c"),
    (".h", "c"),
    (".dart", "dart"),
    (".lua", "lua"),
    (".sql", "sql"),
    (".proto", "protobuf"),
];

const GITIGNORE_ENTRIES: &[&str] = &[
    ".devtribunal_agents/",
    ".claude/commands/dt/",
];

fn detect_languages(repo_path: &Path) -> HashSet<String> {
    let mut languages = HashSet::new();
    let dirs_to_scan = [
        repo_path.to_path_buf(),
        repo_path.join("src"),
        repo_path.join("lib"),
        repo_path.join("app"),
        repo_path.join("cmd"),
        repo_path.join("internal"),
        repo_path.join("packages"),
        repo_path.join("test"),
        repo_path.join("tests"),
    ];

    for dir in &dirs_to_scan {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let dot_ext = format!(".{ext}");
                for &(pattern, lang) in EXTENSION_TO_LANGUAGE {
                    if pattern == dot_ext {
                        languages.insert(lang.to_string());
                    }
                }
            }
        }
    }

    languages
}

fn ensure_gitignore(repo_path: &Path, entries: &[&str]) -> Vec<String> {
    let gitignore_path = repo_path.join(".gitignore");
    let existing = std::fs::read_to_string(&gitignore_path).unwrap_or_default();
    let existing_lines: Vec<&str> = existing.lines().map(|l| l.trim()).collect();

    let to_add: Vec<&&str> = entries
        .iter()
        .filter(|e| !existing_lines.contains(&e.trim()))
        .collect();

    if to_add.is_empty() {
        return Vec::new();
    }

    let mut addition = String::new();
    if !existing.is_empty() && !existing.ends_with('\n') {
        addition.push('\n');
    }
    addition.push_str("\n# devtribunal (remove these lines to version-control agents and skills)\n");
    for entry in &to_add {
        addition.push_str(entry);
        addition.push('\n');
    }

    let new_content = format!("{existing}{addition}");
    let _ = std::fs::write(&gitignore_path, new_content);

    to_add.iter().map(|e| e.to_string()).collect()
}

fn scaffold_skills(repo_path: &Path) -> (Vec<String>, usize, usize) {
    let target_dir = repo_path.join(".claude").join("commands").join("dt");
    let mut results = Vec::new();
    let mut written = 0;
    let mut skipped = 0;

    if std::fs::create_dir_all(&target_dir).is_err() {
        return (vec!["  Failed to create skills directory".to_string()], 0, 0);
    }

    for &(filename, content) in embedded_skills() {
        let target_path = target_dir.join(filename);

        if target_path.exists() {
            let existing = std::fs::read_to_string(&target_path).unwrap_or_default();
            if existing.trim() == content.trim() {
                results.push(format!("  SKIPPED {filename} (already current)"));
                skipped += 1;
                continue;
            }
            results.push(format!("  SKIPPED {filename} (modified by user — won't overwrite)"));
            skipped += 1;
            continue;
        }

        match std::fs::write(&target_path, content) {
            Ok(_) => {
                results.push(format!("  WROTE   {filename}"));
                written += 1;
            }
            Err(e) => {
                results.push(format!("  ERROR   {filename}: {e}"));
            }
        }
    }

    (results, written, skipped)
}

pub fn execute_init(repo_path: &str, languages: Option<&[String]>) -> ToolResult {
    let repo = Path::new(repo_path);
    let target_dir = repo.join(".devtribunal_agents");

    // Detect or use provided languages
    let detected: HashSet<String> = match languages {
        Some(langs) => langs.iter().cloned().collect(),
        None => detect_languages(repo),
    };

    if detected.is_empty() {
        return ToolResult {
            content: "No supported languages detected in this repository. \
                You can specify languages explicitly: { languages: [\"typescript\", \"python\"] }"
                .to_string(),
            is_error: false,
        };
    }

    // Filter embedded agents to relevant ones
    let mut relevant: Vec<(&str, &str)> = Vec::new();
    for &(filename, raw) in crate::types::EMBEDDED_AGENTS {
        if let Ok(agent) = parse_agent(filename, raw) {
            let is_language_agnostic = agent.languages.is_empty();
            let matches_language = agent.languages.iter().any(|l| detected.contains(l));
            if is_language_agnostic || matches_language {
                relevant.push((filename, raw));
            }
        }
    }

    if relevant.is_empty() {
        return ToolResult {
            content: format!(
                "Detected languages: {}. No matching agents available yet.",
                detected.iter().cloned().collect::<Vec<_>>().join(", ")
            ),
            is_error: false,
        };
    }

    // Create target directory
    if let Err(e) = std::fs::create_dir_all(&target_dir) {
        return ToolResult {
            content: format!("Cannot create directory {}: {e}", target_dir.display()),
            is_error: true,
        };
    }

    let mut results = Vec::new();
    let mut written = 0;
    let mut skipped = 0;

    for (filename, raw) in &relevant {
        let target_path = target_dir.join(filename);

        if target_path.exists() {
            let existing_raw = match std::fs::read_to_string(&target_path) {
                Ok(s) => s,
                Err(_) => {
                    results.push(format!("  SKIPPED {filename} (cannot read existing)"));
                    skipped += 1;
                    continue;
                }
            };

            // Check source: custom
            if let Ok(agent) = parse_agent(filename, &existing_raw) {
                if agent.source.as_deref() == Some("custom") {
                    results.push(format!("  SKIPPED {filename} (source: custom — user-created)"));
                    skipped += 1;
                    continue;
                }
            }

            if existing_raw.trim() == raw.trim() {
                results.push(format!("  SKIPPED {filename} (already current)"));
                skipped += 1;
                continue;
            }

            results.push(format!("  SKIPPED {filename} (modified by user — won't overwrite)"));
            skipped += 1;
            continue;
        }

        match std::fs::write(&target_path, raw) {
            Ok(_) => {
                results.push(format!("  WROTE   {filename}"));
                written += 1;
            }
            Err(e) => {
                results.push(format!("  ERROR   {filename}: {e}"));
            }
        }
    }

    // Scaffold skills
    let (skill_results, skill_written, skill_skipped) = scaffold_skills(repo);

    // Update .gitignore if anything was written
    let total_written = written + skill_written;
    let mut gitignore_added = Vec::new();
    if total_written > 0 {
        gitignore_added = ensure_gitignore(repo, GITIGNORE_ENTRIES);
    }

    let mut langs_sorted: Vec<_> = detected.into_iter().collect();
    langs_sorted.sort();

    let mut summary = vec![
        format!("Languages detected: {}", langs_sorted.join(", ")),
        String::new(),
        format!("## Agents → {}", target_dir.display()),
    ];
    summary.extend(results);
    summary.push(format!("{written} written, {skipped} skipped"));
    summary.push(String::new());
    summary.push(format!(
        "## Skills → {}",
        repo.join(".claude").join("commands").join("dt").display()
    ));
    summary.extend(skill_results);
    summary.push(format!("{skill_written} written, {skill_skipped} skipped"));

    if !gitignore_added.is_empty() {
        summary.push(String::new());
        summary.push("## .gitignore".to_string());
        summary.push(format!("  Added: {}", gitignore_added.join(", ")));
        summary.push("  These paths are gitignored by default (no trace in your repo).".to_string());
    }

    if total_written > 0 {
        summary.extend([
            String::new(),
            "You can:".to_string(),
            "  - Edit agent files to customize review criteria for your team".to_string(),
            "  - Use /dt:full, /dt:incremental-pr-ready, /dt:incremental-staged, /dt:incremental-wip".to_string(),
            "  - To version-control your agents and skills, remove the devtribunal lines from .gitignore".to_string(),
            "  - Add source: custom to agent frontmatter for files you create from scratch".to_string(),
        ]);
    }

    ToolResult {
        content: summary.join("\n"),
        is_error: false,
    }
}
