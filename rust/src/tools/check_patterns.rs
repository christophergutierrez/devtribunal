use std::collections::{HashMap, HashSet};
use std::path::Path;

use ignore::WalkBuilder;
use regex::Regex;

use super::ToolResult;
use crate::lang::{language_for_path, SKIP_DIRS, SOURCE_EXTENSIONS};

#[derive(Debug, Clone)]
struct FileMetadata {
    path: String,
    language: String,
    imports: Vec<ImportStatement>,
    exports: Vec<String>,
    error_patterns: Vec<ErrorPattern>,
    string_literals: Vec<(String, u32)>,
    content: String,
}

#[derive(Debug, Clone)]
struct ImportStatement {
    source: String,
    symbols: Vec<String>,
}

#[derive(Debug, Clone)]
enum ErrorPattern {
    Unwrap,
    QuestionMark,
    Expect,
    TryCatch,
    PanicExplicit,
}

impl ErrorPattern {
    fn label(&self) -> &'static str {
        match self {
            Self::Unwrap => "unwrap()",
            Self::QuestionMark => "? operator",
            Self::Expect => "expect()",
            Self::TryCatch => "try/catch",
            Self::PanicExplicit => "panic!()",
        }
    }
}

struct Cycle {
    files: Vec<String>,
}

struct DeadExport {
    file: String,
    symbol: String,
}

struct ErrorInconsistency {
    module_path: String,
    files: Vec<(String, Vec<String>)>,
}

struct DuplicatedLiteral {
    value: String,
    occurrences: Vec<(String, u32)>,
}

struct PatternAnalysisResult {
    cycles: Vec<Cycle>,
    dead_exports: Vec<DeadExport>,
    error_inconsistencies: Vec<ErrorInconsistency>,
    duplicated_literals: Vec<DuplicatedLiteral>,
    files_analyzed: usize,
    errors: Vec<String>,
}

// --- Import/export extraction ---

fn extract_imports(content: &str, lang: &str) -> Vec<ImportStatement> {
    match lang {
        "rust" => extract_rust_imports(content),
        "typescript" | "javascript" => extract_ts_imports(content),
        "python" => extract_python_imports(content),
        "go" => extract_go_imports(content),
        _ => Vec::new(),
    }
}

fn extract_rust_imports(content: &str) -> Vec<ImportStatement> {
    let re = Regex::new(r"use\s+([\w:]+)(?:::\{([^}]+)\})?;").unwrap();
    let mut imports = Vec::new();

    for cap in re.captures_iter(content) {
        let source = cap[1].to_string();
        let symbols = if let Some(group) = cap.get(2) {
            group.as_str().split(',').map(|s| s.trim().to_string()).collect()
        } else {
            let last = source.rsplit("::").next().unwrap_or("").to_string();
            if last.is_empty() { Vec::new() } else { vec![last] }
        };
        imports.push(ImportStatement { source, symbols });
    }

    imports
}

fn extract_ts_imports(content: &str) -> Vec<ImportStatement> {
    let re = Regex::new(r#"import\s+\{([^}]+)\}\s+from\s+['"]([^'"]+)['"]"#).unwrap();
    let re_default = Regex::new(r#"import\s+(\w+)\s+from\s+['"]([^'"]+)['"]"#).unwrap();
    let mut imports = Vec::new();

    for cap in re.captures_iter(content) {
        let symbols: Vec<String> = cap[1].split(',').map(|s| {
            let s = s.trim();
            s.split(" as ").next().unwrap_or(s).trim().to_string()
        }).collect();
        imports.push(ImportStatement { source: cap[2].to_string(), symbols });
    }

    for cap in re_default.captures_iter(content) {
        imports.push(ImportStatement {
            source: cap[2].to_string(),
            symbols: vec![cap[1].to_string()],
        });
    }

    imports
}

fn extract_python_imports(content: &str) -> Vec<ImportStatement> {
    let re_from = Regex::new(r"from\s+([\w.]+)\s+import\s+(.+)").unwrap();
    let re_import = Regex::new(r"^import\s+([\w.]+)").unwrap();
    let mut imports = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(cap) = re_from.captures(trimmed) {
            let symbols: Vec<String> = cap[2].split(',')
                .map(|s| s.split(" as ").next().unwrap_or("").trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            imports.push(ImportStatement { source: cap[1].to_string(), symbols });
        } else if let Some(cap) = re_import.captures(trimmed) {
            imports.push(ImportStatement { source: cap[1].to_string(), symbols: Vec::new() });
        }
    }

    imports
}

fn extract_go_imports(content: &str) -> Vec<ImportStatement> {
    let re_single = Regex::new(r#"import\s+"([^"]+)""#).unwrap();
    let re_block = Regex::new(r#"import\s*\(([\s\S]*?)\)"#).unwrap();
    let re_path = Regex::new(r#""([^"]+)""#).unwrap();
    let mut imports = Vec::new();

    for cap in re_single.captures_iter(content) {
        imports.push(ImportStatement { source: cap[1].to_string(), symbols: Vec::new() });
    }

    for cap in re_block.captures_iter(content) {
        for path_cap in re_path.captures_iter(&cap[1]) {
            imports.push(ImportStatement { source: path_cap[1].to_string(), symbols: Vec::new() });
        }
    }

    imports
}

fn extract_exports(content: &str, lang: &str) -> Vec<String> {
    let patterns: Vec<Regex> = match lang {
        "rust" => vec![
            Regex::new(r"pub\s+(?:async\s+)?fn\s+(\w+)").unwrap(),
            Regex::new(r"pub\s+struct\s+(\w+)").unwrap(),
            Regex::new(r"pub\s+enum\s+(\w+)").unwrap(),
            Regex::new(r"pub\s+trait\s+(\w+)").unwrap(),
            Regex::new(r"pub\s+type\s+(\w+)").unwrap(),
            Regex::new(r"pub\s+const\s+(\w+)").unwrap(),
            Regex::new(r"pub\s+static\s+(\w+)").unwrap(),
        ],
        "typescript" | "javascript" => vec![
            Regex::new(r"export\s+(?:async\s+)?function\s+(\w+)").unwrap(),
            Regex::new(r"export\s+class\s+(\w+)").unwrap(),
            Regex::new(r"export\s+(?:const|let|var)\s+(\w+)").unwrap(),
            Regex::new(r"export\s+interface\s+(\w+)").unwrap(),
            Regex::new(r"export\s+type\s+(\w+)").unwrap(),
            Regex::new(r"export\s+enum\s+(\w+)").unwrap(),
        ],
        "python" => vec![
            Regex::new(r"^def\s+(\w+)").unwrap(),
            Regex::new(r"^class\s+(\w+)").unwrap(),
        ],
        "go" => vec![
            Regex::new(r"^func\s+(?:\([^)]*\)\s+)?([A-Z]\w*)").unwrap(),
            Regex::new(r"^type\s+([A-Z]\w*)").unwrap(),
        ],
        _ => Vec::new(),
    };

    let mut exports = Vec::new();
    for re in &patterns {
        for cap in re.captures_iter(content) {
            if let Some(name) = cap.get(1) {
                exports.push(name.as_str().to_string());
            }
        }
    }
    exports
}

fn extract_error_patterns(content: &str, lang: &str) -> Vec<ErrorPattern> {
    let mut patterns = Vec::new();

    match lang {
        "rust" => {
            if content.contains(".unwrap()") { patterns.push(ErrorPattern::Unwrap); }
            if content.contains(".expect(") { patterns.push(ErrorPattern::Expect); }
            if Regex::new(r"\?\s*;|\?\s*$").unwrap().is_match(content) { patterns.push(ErrorPattern::QuestionMark); }
            if content.contains("panic!(") { patterns.push(ErrorPattern::PanicExplicit); }
        }
        "typescript" | "javascript" | "java" | "csharp" | "php" => {
            if content.contains("try {") || content.contains("try\n") { patterns.push(ErrorPattern::TryCatch); }
            if content.contains(".unwrap()") { patterns.push(ErrorPattern::Unwrap); } // Rust-in-TS libs
        }
        "go" => {
            if content.contains("panic(") { patterns.push(ErrorPattern::PanicExplicit); }
        }
        "python" => {
            if content.contains("try:") { patterns.push(ErrorPattern::TryCatch); }
        }
        _ => {}
    }

    patterns
}

fn extract_string_literals(content: &str, lang: &str) -> Vec<(String, u32)> {
    let re = match lang {
        "rust" => Regex::new(r#""([^"\\]{8,})""#).unwrap(),
        "python" => Regex::new(r#"(?:"|')([^"'\\]{8,})(?:"|')"#).unwrap(),
        _ => Regex::new(r#"(?:"|'|`)([^"'`\\]{8,})(?:"|'|`)"#).unwrap(),
    };

    let noise: HashSet<&str> = [
        "utf-8", "utf8", "ascii", "application/json", "text/html",
        "Content-Type", "content-type", "localhost", "127.0.0.1",
        "GET", "POST", "PUT", "DELETE", "PATCH",
    ].into_iter().collect();

    let mut literals = Vec::new();
    for (line_num, line) in content.lines().enumerate() {
        // Skip comment lines
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("/*") {
            continue;
        }
        for cap in re.captures_iter(line) {
            let val = cap[1].to_string();
            if !noise.contains(val.as_str()) && !val.starts_with("http") {
                literals.push((val, (line_num + 1) as u32));
            }
        }
    }

    literals
}

// --- Cycle detection (Tarjan's SCC) ---

fn detect_cycles(files: &[FileMetadata], repo_path: &str) -> Vec<Cycle> {
    let file_set: HashMap<&str, usize> = files.iter().enumerate()
        .map(|(i, f)| (f.path.as_str(), i))
        .collect();

    // Build adjacency list
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); files.len()];
    for (i, file) in files.iter().enumerate() {
        for import in &file.imports {
            let resolved = resolve_import(&import.source, &file.path, &file.language, repo_path);
            for target in &resolved {
                if let Some(&j) = file_set.get(target.as_str()) {
                    if i != j {
                        adj[i].push(j);
                    }
                }
            }
        }
    }

    // Tarjan's SCC
    let sccs = tarjan_scc(&adj);
    sccs.into_iter()
        .filter(|scc| scc.len() > 1)
        .map(|scc| Cycle {
            files: scc.into_iter().map(|i| files[i].path.clone()).collect(),
        })
        .collect()
}

fn resolve_import(source: &str, current_file: &str, lang: &str, _repo_path: &str) -> Vec<String> {
    match lang {
        "rust" => {
            // crate::foo::bar -> src/foo/bar.rs or src/foo.rs
            if let Some(rest) = source.strip_prefix("crate::") {
                let parts: Vec<&str> = rest.split("::").collect();
                let mut candidates = Vec::new();
                // Try as file: src/foo/bar.rs
                let path = format!("rust/src/{}.rs", parts.join("/"));
                candidates.push(path);
                // Try as module: src/foo/bar/mod.rs
                if parts.len() > 1 {
                    let dir_path = format!("rust/src/{}/mod.rs", parts[..parts.len()-1].join("/"));
                    candidates.push(dir_path);
                }
                return candidates;
            }
            Vec::new()
        }
        "typescript" | "javascript" => {
            if source.starts_with('.') {
                let current_dir = current_file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                let resolved = if let Some(rest) = source.strip_prefix("./") {
                    format!("{}/{}", current_dir, rest)
                } else if let Some(rest) = source.strip_prefix("../") {
                    let parent = current_dir.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                    format!("{}/{}", parent, rest)
                } else {
                    return Vec::new();
                };
                // Try with extensions
                vec![
                    format!("{resolved}.ts"),
                    format!("{resolved}.tsx"),
                    format!("{resolved}.js"),
                    format!("{resolved}/index.ts"),
                    format!("{resolved}/index.js"),
                ]
            } else {
                Vec::new() // external package
            }
        }
        "python" => {
            if source.starts_with('.') {
                let current_dir = current_file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                let module = source.trim_start_matches('.');
                vec![format!("{current_dir}/{}.py", module.replace('.', "/"))]
            } else {
                vec![format!("{}.py", source.replace('.', "/"))]
            }
        }
        _ => Vec::new(),
    }
}

fn tarjan_scc(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = adj.len();
    let mut index_counter = 0u32;
    let mut scc_stack: Vec<usize> = Vec::new();
    let mut on_stack = vec![false; n];
    let mut index = vec![u32::MAX; n];
    let mut lowlink = vec![0u32; n];
    let mut result = Vec::new();

    // Iterative Tarjan's using an explicit call stack to avoid
    // stack overflow on large graphs (1000+ nodes).
    struct Frame {
        v: usize,
        neighbor_idx: usize,
    }

    for start in 0..n {
        if index[start] != u32::MAX {
            continue;
        }

        let mut call_stack: Vec<Frame> = Vec::new();

        // Initialize the start node
        index[start] = index_counter;
        lowlink[start] = index_counter;
        index_counter += 1;
        scc_stack.push(start);
        on_stack[start] = true;
        call_stack.push(Frame { v: start, neighbor_idx: 0 });

        while let Some(frame) = call_stack.last_mut() {
            let v = frame.v;
            if frame.neighbor_idx < adj[v].len() {
                let w = adj[v][frame.neighbor_idx];
                frame.neighbor_idx += 1;

                if index[w] == u32::MAX {
                    // "Recurse" into w: initialize it and push a new frame
                    index[w] = index_counter;
                    lowlink[w] = index_counter;
                    index_counter += 1;
                    scc_stack.push(w);
                    on_stack[w] = true;
                    call_stack.push(Frame { v: w, neighbor_idx: 0 });
                } else if on_stack[w] {
                    lowlink[v] = lowlink[v].min(index[w]);
                }
            } else {
                // All neighbors processed -- equivalent to returning from recursion
                let v = frame.v;
                call_stack.pop();

                // Update parent's lowlink (equivalent to post-recursion update)
                if let Some(parent_frame) = call_stack.last() {
                    let parent = parent_frame.v;
                    lowlink[parent] = lowlink[parent].min(lowlink[v]);
                }

                // Check if v is the root of an SCC
                if lowlink[v] == index[v] {
                    let mut scc = Vec::new();
                    while let Some(w) = scc_stack.pop() {
                        on_stack[w] = false;
                        scc.push(w);
                        if w == v {
                            break;
                        }
                    }
                    result.push(scc);
                }
            }
        }
    }

    result
}

// --- Dead export detection ---

fn find_dead_exports(files: &[FileMetadata]) -> Vec<DeadExport> {
    // Collect all imported symbols across all files
    let mut all_imported: HashSet<String> = HashSet::new();
    for file in files {
        for import in &file.imports {
            for sym in &import.symbols {
                all_imported.insert(sym.clone());
            }
        }
    }

    let mut dead = Vec::new();
    for file in files {
        // Skip entry points and test files
        let filename = file.path.rsplit('/').next().unwrap_or(&file.path);
        if filename == "main.rs" || filename == "main.go" || filename == "main.py"
            || filename == "mod.rs" || filename == "index.ts" || filename == "index.js"
            || filename.contains("test") || filename.contains("spec")
            || filename == "__init__.py"
        {
            continue;
        }

        for export in &file.exports {
            if !all_imported.contains(export) {
                // For Rust files, do a secondary grep-based check.
                // Rust pub items can be used via path (crate::module::symbol) without
                // an explicit `use` statement, causing false positives with import-only detection.
                if file.language == "rust" {
                    let re = match Regex::new(&format!(r"\b{}\b", regex::escape(export))) {
                        Ok(r) => r,
                        Err(_) => continue,
                    };
                    let found_elsewhere = files.iter().any(|other| {
                        other.path != file.path && re.is_match(&other.content)
                    });
                    if found_elsewhere {
                        continue;
                    }
                }

                dead.push(DeadExport {
                    file: file.path.clone(),
                    symbol: export.clone(),
                });
            }
        }
    }

    dead
}

// --- Error inconsistency detection ---

fn find_error_inconsistencies(files: &[FileMetadata]) -> Vec<ErrorInconsistency> {
    // Group files by parent directory
    let mut by_dir: HashMap<String, Vec<&FileMetadata>> = HashMap::new();
    for file in files {
        let dir = file.path.rsplit_once('/')
            .map(|(d, _)| d.to_string())
            .unwrap_or_else(|| ".".to_string());
        by_dir.entry(dir).or_default().push(file);
    }

    let mut inconsistencies = Vec::new();

    for (dir, dir_files) in &by_dir {
        if dir_files.len() < 2 {
            continue;
        }

        let mut pattern_sets: Vec<(String, Vec<String>)> = Vec::new();
        for file in dir_files {
            if file.error_patterns.is_empty() {
                continue;
            }
            let labels: Vec<String> = file.error_patterns.iter()
                .map(|p| p.label().to_string())
                .collect();
            pattern_sets.push((file.path.clone(), labels));
        }

        if pattern_sets.len() < 2 {
            continue;
        }

        // Check if files use fundamentally different patterns
        let first_patterns: HashSet<&str> = pattern_sets[0].1.iter().map(|s| s.as_str()).collect();
        let mut inconsistent = false;
        for (_, patterns) in &pattern_sets[1..] {
            let these: HashSet<&str> = patterns.iter().map(|s| s.as_str()).collect();
            if first_patterns.is_disjoint(&these) {
                inconsistent = true;
                break;
            }
        }

        if inconsistent {
            inconsistencies.push(ErrorInconsistency {
                module_path: dir.clone(),
                files: pattern_sets,
            });
        }
    }

    inconsistencies
}

// --- Duplicated literal detection ---

fn find_duplicated_literals(files: &[FileMetadata]) -> Vec<DuplicatedLiteral> {
    let mut literal_map: HashMap<String, Vec<(String, u32)>> = HashMap::new();

    for file in files {
        for (val, line) in &file.string_literals {
            literal_map.entry(val.clone())
                .or_default()
                .push((file.path.clone(), *line));
        }
    }

    let mut duplicated: Vec<DuplicatedLiteral> = literal_map.into_iter()
        .filter(|(_, occurrences)| {
            let unique_files: HashSet<&str> = occurrences.iter().map(|(f, _)| f.as_str()).collect();
            unique_files.len() >= 3
        })
        .map(|(value, occurrences)| DuplicatedLiteral { value, occurrences })
        .collect();

    duplicated.sort_by(|a, b| b.occurrences.len().cmp(&a.occurrences.len()));
    duplicated.truncate(20);
    duplicated
}

// --- Output formatting ---

fn format_pattern_analysis(result: &PatternAnalysisResult) -> String {
    let mut lines = vec![
        "# Cross-File Pattern Analysis".to_string(),
        String::new(),
        format!("**Files analyzed:** {}", result.files_analyzed),
    ];

    let total_findings = result.cycles.len() + result.dead_exports.len()
        + result.error_inconsistencies.len() + result.duplicated_literals.len();

    if total_findings == 0 {
        lines.push(String::new());
        lines.push("No cross-file pattern issues detected.".to_string());
        return lines.join("\n");
    }

    if !result.cycles.is_empty() {
        lines.push(String::new());
        lines.push(format!("## Circular Dependencies ({} cycles)", result.cycles.len()));
        for (i, cycle) in result.cycles.iter().enumerate() {
            lines.push(String::new());
            lines.push(format!("### Cycle {} ({} files)", i + 1, cycle.files.len()));
            let cycle_str = cycle.files.iter()
                .map(|f| format!("`{f}`"))
                .collect::<Vec<_>>()
                .join(" -> ");
            lines.push(format!("{cycle_str} -> `{}`", cycle.files[0]));
        }
    }

    if !result.dead_exports.is_empty() {
        lines.push(String::new());
        lines.push(format!("## Dead Exports ({} symbols)", result.dead_exports.len()));
        lines.push("| Symbol | File |".to_string());
        lines.push("|--------|------|".to_string());
        for d in result.dead_exports.iter().take(30) {
            lines.push(format!("| `{}` | `{}` |", d.symbol, d.file));
        }
        if result.dead_exports.len() > 30 {
            lines.push(format!("| ... | +{} more |", result.dead_exports.len() - 30));
        }
    }

    if !result.error_inconsistencies.is_empty() {
        lines.push(String::new());
        lines.push(format!("## Inconsistent Error Handling ({} modules)", result.error_inconsistencies.len()));
        for inc in &result.error_inconsistencies {
            lines.push(String::new());
            lines.push(format!("### `{}/`", inc.module_path));
            for (file, patterns) in &inc.files {
                let filename = file.rsplit('/').next().unwrap_or(file);
                lines.push(format!("- `{filename}` — uses: {}", patterns.join(", ")));
            }
        }
    }

    if !result.duplicated_literals.is_empty() {
        lines.push(String::new());
        lines.push(format!("## Duplicated Magic Strings ({} strings)", result.duplicated_literals.len()));
        lines.push("| String | Files |".to_string());
        lines.push("|--------|-------|".to_string());
        for d in &result.duplicated_literals {
            let display_val = if d.value.len() > 40 {
                format!("{}...", &d.value[..37])
            } else {
                d.value.clone()
            };
            let unique_files: HashSet<&str> = d.occurrences.iter().map(|(f, _)| f.as_str()).collect();
            lines.push(format!("| `{display_val}` | {} files |", unique_files.len()));
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

// --- Main execution ---

const MAX_FILES: usize = 5000;
const MAX_FILE_SIZE: usize = 100 * 1024; // 100KB

pub async fn execute_check_patterns(repo_path: &str, languages: Option<&[String]>) -> ToolResult {
    let repo = Path::new(repo_path);
    if !repo.exists() {
        return ToolResult {
            content: format!("Path does not exist: {repo_path}"),
            is_error: true,
        };
    }

    let mut errors = Vec::new();
    let mut files_metadata: Vec<FileMetadata> = Vec::new();

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

    let mut file_count = 0;
    for entry in walker.flatten() {
        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }

        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !SOURCE_EXTENSIONS.contains(&ext) {
            continue;
        }

        let rel_path = path.strip_prefix(repo)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let lang = match language_for_path(&rel_path) {
            Some(l) => l,
            None => continue,
        };

        if let Some(filter) = languages {
            if !filter.iter().any(|f| f == lang) {
                continue;
            }
        }

        file_count += 1;
        if file_count > MAX_FILES {
            errors.push(format!("Stopped at {MAX_FILES} files — repo is very large"));
            break;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if content.len() > MAX_FILE_SIZE {
            continue;
        }

        let imports = extract_imports(&content, lang);
        let exports = extract_exports(&content, lang);
        let error_patterns = extract_error_patterns(&content, lang);
        let string_literals = extract_string_literals(&content, lang);

        files_metadata.push(FileMetadata {
            path: rel_path,
            language: lang.to_string(),
            imports,
            exports,
            error_patterns,
            string_literals,
            content,
        });
    }

    // Analyze patterns
    let cycles = detect_cycles(&files_metadata, repo_path);
    let dead_exports = find_dead_exports(&files_metadata);
    let error_inconsistencies = find_error_inconsistencies(&files_metadata);
    let duplicated_literals = find_duplicated_literals(&files_metadata);

    let result = PatternAnalysisResult {
        cycles,
        dead_exports,
        error_inconsistencies,
        duplicated_literals,
        files_analyzed: files_metadata.len(),
        errors,
    };

    ToolResult {
        content: format_pattern_analysis(&result),
        is_error: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_rust_imports() {
        let content = "use crate::types::AgentDefinition;\nuse std::collections::HashMap;\nuse crate::tools::{review, orchestrate};\n";
        let imports = extract_rust_imports(content);
        assert!(imports.len() >= 2);
        assert!(imports.iter().any(|i| i.source.contains("crate::types")));
        assert!(imports.iter().any(|i| i.symbols.contains(&"review".to_string())));
    }

    #[test]
    fn test_extract_ts_imports() {
        let content = r#"import { useState, useEffect } from 'react';
import { Config } from './config';
import express from 'express';
"#;
        let imports = extract_ts_imports(content);
        assert!(imports.iter().any(|i| i.source == "react" && i.symbols.contains(&"useState".to_string())));
        assert!(imports.iter().any(|i| i.source == "./config"));
        assert!(imports.iter().any(|i| i.source == "express"));
    }

    #[test]
    fn test_extract_python_imports() {
        let content = "from flask import Flask, request\nimport os\nfrom .utils import helper\n";
        let imports = extract_python_imports(content);
        assert!(imports.iter().any(|i| i.source == "flask" && i.symbols.contains(&"Flask".to_string())));
        assert!(imports.iter().any(|i| i.source == "os"));
        assert!(imports.iter().any(|i| i.source == ".utils"));
    }

    #[test]
    fn test_detect_cycles_simple() {
        let files = vec![
            FileMetadata {
                path: "src/a.ts".into(), language: "typescript".into(),
                imports: vec![ImportStatement { source: "./b".into(), symbols: vec!["B".into()] }],
                exports: vec!["A".into()], error_patterns: Vec::new(), string_literals: Vec::new(),
                content: String::new(),
            },
            FileMetadata {
                path: "src/b.ts".into(), language: "typescript".into(),
                imports: vec![ImportStatement { source: "./a".into(), symbols: vec!["A".into()] }],
                exports: vec!["B".into()], error_patterns: Vec::new(), string_literals: Vec::new(),
                content: String::new(),
            },
        ];
        let cycles = detect_cycles(&files, "/tmp/repo");
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].files.len(), 2);
    }

    #[test]
    fn test_find_dead_exports() {
        let files = vec![
            FileMetadata {
                path: "src/utils.ts".into(), language: "typescript".into(),
                imports: Vec::new(),
                exports: vec!["usedFunc".into(), "deadFunc".into()],
                error_patterns: Vec::new(), string_literals: Vec::new(),
                content: "export function usedFunc() {}\nexport function deadFunc() {}".into(),
            },
            FileMetadata {
                path: "src/app.ts".into(), language: "typescript".into(),
                imports: vec![ImportStatement { source: "./utils".into(), symbols: vec!["usedFunc".into()] }],
                exports: Vec::new(),
                error_patterns: Vec::new(), string_literals: Vec::new(),
                content: "import { usedFunc } from './utils';".into(),
            },
        ];
        let dead = find_dead_exports(&files);
        assert!(dead.iter().any(|d| d.symbol == "deadFunc"));
        assert!(!dead.iter().any(|d| d.symbol == "usedFunc"));
    }

    #[test]
    fn test_find_dead_exports_rust_path_usage() {
        // In Rust, a pub symbol can be used via crate::module::symbol without a `use` statement.
        // The grep-based fallback should prevent false positives in this case.
        let files = vec![
            FileMetadata {
                path: "src/utils.rs".into(), language: "rust".into(),
                imports: Vec::new(),
                exports: vec!["helper_func".into(), "truly_dead".into()],
                error_patterns: Vec::new(), string_literals: Vec::new(),
                content: "pub fn helper_func() {}\npub fn truly_dead() {}".into(),
            },
            FileMetadata {
                path: "src/app.rs".into(), language: "rust".into(),
                imports: Vec::new(), // No explicit `use` statement
                exports: Vec::new(),
                error_patterns: Vec::new(), string_literals: Vec::new(),
                // Uses helper_func via path without importing it
                content: "fn main() {\n    crate::utils::helper_func();\n}".into(),
            },
        ];
        let dead = find_dead_exports(&files);
        // helper_func is referenced in app.rs via path, so it should NOT be dead
        assert!(!dead.iter().any(|d| d.symbol == "helper_func"));
        // truly_dead is not referenced anywhere else, so it IS dead
        assert!(dead.iter().any(|d| d.symbol == "truly_dead"));
    }

    #[test]
    fn test_duplicated_literals() {
        let files = vec![
            FileMetadata {
                path: "a.rs".into(), language: "rust".into(),
                imports: Vec::new(), exports: Vec::new(), error_patterns: Vec::new(),
                string_literals: vec![("some_magic_string".into(), 1)],
                content: String::new(),
            },
            FileMetadata {
                path: "b.rs".into(), language: "rust".into(),
                imports: Vec::new(), exports: Vec::new(), error_patterns: Vec::new(),
                string_literals: vec![("some_magic_string".into(), 5)],
                content: String::new(),
            },
            FileMetadata {
                path: "c.rs".into(), language: "rust".into(),
                imports: Vec::new(), exports: Vec::new(), error_patterns: Vec::new(),
                string_literals: vec![("some_magic_string".into(), 10)],
                content: String::new(),
            },
        ];
        let dupes = find_duplicated_literals(&files);
        assert_eq!(dupes.len(), 1);
        assert_eq!(dupes[0].value, "some_magic_string");
        assert_eq!(dupes[0].occurrences.len(), 3);
    }
}
