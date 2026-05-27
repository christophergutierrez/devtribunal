use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use ignore::WalkBuilder;

use super::ToolResult;
use crate::lang::{language_for_path, SKIP_DIRS, SOURCE_EXTENSIONS};
use crate::shell::safe_exec_in_dir;

const MAX_FILES: usize = 5000;
const MAX_FILE_SIZE: usize = 100 * 1024; // 100KB

// --- File classification ---

#[derive(Debug, Clone, PartialEq)]
enum FileKind {
    Source,
    TestFile,
}

#[derive(Debug, Clone)]
struct ClassifiedFile {
    rel_path: String,
    language: String,
    kind: FileKind,
    has_inline_tests: bool,
}

/// Determine if a file is a test file based on language conventions.
fn classify_file(rel_path: &str, language: &str, content: &str) -> ClassifiedFile {
    let filename = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let is_test = match language {
        "rust" => {
            // Files in tests/ directory or named *_test.rs
            rel_path.starts_with("tests/")
                || rel_path.contains("/tests/")
                || filename.ends_with("_test.rs")
        }
        "typescript" | "javascript" => {
            filename.ends_with(".test.ts")
                || filename.ends_with(".spec.ts")
                || filename.ends_with(".test.js")
                || filename.ends_with(".spec.js")
                || filename.ends_with(".test.tsx")
                || filename.ends_with(".spec.tsx")
                || filename.ends_with(".test.jsx")
                || filename.ends_with(".spec.jsx")
                || rel_path.contains("__tests__/")
        }
        "python" => {
            filename.starts_with("test_")
                || filename.ends_with("_test.py")
                || rel_path.starts_with("tests/")
                || rel_path.contains("/tests/")
                || rel_path.starts_with("test/")
                || rel_path.contains("/test/")
        }
        "go" => filename.ends_with("_test.go"),
        "java" => {
            rel_path.contains("src/test/")
                || filename.ends_with("Test.java")
                || filename.ends_with("Tests.java")
        }
        "php" => {
            filename.ends_with("Test.php")
                || rel_path.starts_with("tests/")
                || rel_path.contains("/tests/")
        }
        _ => false,
    };

    let has_inline_tests = match language {
        "rust" => content.contains("#[cfg(test)]") || content.contains("#[test]"),
        _ => false,
    };

    ClassifiedFile {
        rel_path: rel_path.to_string(),
        language: language.to_string(),
        kind: if is_test { FileKind::TestFile } else { FileKind::Source },
        has_inline_tests,
    }
}

// --- Test runner detection ---

#[derive(Debug, Clone)]
struct DetectedRunner {
    command: String,
    name: String,
}

fn detect_test_runner(repo_path: &Path) -> Option<DetectedRunner> {
    // Cargo.toml -> cargo test
    if repo_path.join("Cargo.toml").exists() {
        return Some(DetectedRunner {
            command: "cargo test".to_string(),
            name: "cargo test".to_string(),
        });
    }

    // go.mod -> go test ./...
    if repo_path.join("go.mod").exists() {
        return Some(DetectedRunner {
            command: "go test ./...".to_string(),
            name: "go test".to_string(),
        });
    }

    // package.json with test script
    if repo_path.join("package.json").exists() {
        if let Ok(content) = std::fs::read_to_string(repo_path.join("package.json")) {
            if content.contains("\"test\"") {
                // Determine package manager from lockfile
                let cmd = if repo_path.join("pnpm-lock.yaml").exists() {
                    "pnpm test"
                } else if repo_path.join("yarn.lock").exists() {
                    "yarn test"
                } else {
                    "npm test"
                };
                return Some(DetectedRunner {
                    command: cmd.to_string(),
                    name: cmd.to_string(),
                });
            }
        }
    }

    // pytest
    if repo_path.join("pytest.ini").exists() {
        return Some(DetectedRunner {
            command: "pytest".to_string(),
            name: "pytest".to_string(),
        });
    }
    if repo_path.join("pyproject.toml").exists() {
        if let Ok(content) = std::fs::read_to_string(repo_path.join("pyproject.toml")) {
            if content.contains("[tool.pytest") || content.contains("pytest") {
                return Some(DetectedRunner {
                    command: "pytest".to_string(),
                    name: "pytest".to_string(),
                });
            }
        }
    }

    // Python without pytest
    if repo_path.join("setup.py").exists() || repo_path.join("requirements.txt").exists() {
        return Some(DetectedRunner {
            command: "python -m unittest discover".to_string(),
            name: "unittest".to_string(),
        });
    }

    // Gradle
    if repo_path.join("build.gradle").exists() || repo_path.join("build.gradle.kts").exists() {
        return Some(DetectedRunner {
            command: "./gradlew test".to_string(),
            name: "gradle".to_string(),
        });
    }

    // Maven
    if repo_path.join("pom.xml").exists() {
        return Some(DetectedRunner {
            command: "mvn test".to_string(),
            name: "maven".to_string(),
        });
    }

    None
}

// --- Test result parsing ---

#[derive(Debug, Clone)]
struct TestRunResult {
    status: String,
    passed: u32,
    failed: u32,
    duration: Option<String>,
    failures: Vec<String>,
    raw_output: String,
}

fn parse_cargo_test_output(stdout: &str, stderr: &str, exit_code: i32) -> TestRunResult {
    let combined = format!("{stdout}\n{stderr}");
    let mut passed: u32 = 0;
    let mut failed: u32 = 0;
    let mut failures: Vec<String> = Vec::new();
    let mut duration = None;

    for line in combined.lines() {
        // Match: test result: ok. 48 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.2s
        if line.starts_with("test result:") {
            if let Some(p) = extract_number_before(line, " passed") {
                passed += p;
            }
            if let Some(f) = extract_number_before(line, " failed") {
                failed += f;
            }
            if let Some(idx) = line.find("finished in ") {
                let rest = &line[idx + 12..];
                if let Some(end) = rest.find('s') {
                    duration = Some(format!("{}s", &rest[..end]));
                }
            }
        }
        // Capture failing test names: ---- module::tests::test_name stdout ----
        if line.starts_with("---- ") && line.ends_with(" stdout ----") {
            let test_name = line
                .trim_start_matches("---- ")
                .trim_end_matches(" stdout ----");
            failures.push(test_name.to_string());
        }
    }

    // If we found failure markers, also try to capture assertion messages
    let mut detailed_failures: Vec<String> = Vec::new();
    let lines: Vec<&str> = combined.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("---- ") && line.ends_with(" stdout ----") {
            let test_name = line
                .trim_start_matches("---- ")
                .trim_end_matches(" stdout ----");
            // Look for assertion message in next few lines
            let mut msg = String::new();
            for following in lines.iter().skip(i + 1).take(9) {
                if following.starts_with("---- ") || following.starts_with("failures:") {
                    break;
                }
                let trimmed = following.trim();
                if trimmed.contains("assertion") || trimmed.contains("panicked") {
                    msg = trimmed.to_string();
                    break;
                }
            }
            if msg.is_empty() {
                detailed_failures.push(format!("`{test_name}`"));
            } else {
                detailed_failures.push(format!("`{test_name}` — {msg}"));
            }
        }
    }

    let status = if failed > 0 {
        format!("FAILED ({failed} failure{})", if failed == 1 { "" } else { "s" })
    } else if exit_code != 0 {
        // Non-zero exit with no parsed failures means the runner itself errored
        // (compile error, bad flag, etc.). Never report PASSED in that case.
        format!("FAILED (test runner errored — exit code {exit_code})")
    } else {
        "PASSED".to_string()
    };

    TestRunResult {
        status,
        passed,
        failed,
        duration,
        failures: if detailed_failures.is_empty() { failures.iter().map(|f| format!("`{f}`")).collect() } else { detailed_failures },
        raw_output: combined,
    }
}

fn parse_pytest_output(stdout: &str, stderr: &str, exit_code: i32) -> TestRunResult {
    let combined = format!("{stdout}\n{stderr}");
    let mut passed: u32 = 0;
    let mut failed: u32 = 0;
    let mut duration = None;
    let mut failures: Vec<String> = Vec::new();

    for line in combined.lines() {
        // "= 5 passed in 1.23s ="  or "= 2 failed, 3 passed in 0.5s ="
        if line.contains(" passed") || line.contains(" failed") {
            if let Some(p) = extract_number_before(line, " passed") {
                passed = p;
            }
            if let Some(f) = extract_number_before(line, " failed") {
                failed = f;
            }
            if let Some(idx) = line.find(" in ") {
                let rest = &line[idx + 4..];
                if let Some(end) = rest.find('s') {
                    duration = Some(format!("{}s", &rest[..end]));
                }
            }
        }
        // FAILED test_file.py::test_name
        if line.starts_with("FAILED ") {
            let name = line.trim_start_matches("FAILED ").trim();
            failures.push(format!("`{name}`"));
        }
    }

    let status = if exit_code != 0 || failed > 0 {
        format!("FAILED ({failed} failure{})", if failed == 1 { "" } else { "s" })
    } else {
        "PASSED".to_string()
    };

    TestRunResult {
        status,
        passed,
        failed,
        duration,
        failures,
        raw_output: combined,
    }
}

fn parse_npm_test_output(stdout: &str, stderr: &str, exit_code: i32) -> TestRunResult {
    let combined = format!("{stdout}\n{stderr}");
    let mut passed: u32 = 0;
    let mut failed: u32 = 0;
    let mut duration = None;
    let mut failures: Vec<String> = Vec::new();

    for line in combined.lines() {
        // Jest: Tests:       2 failed, 48 passed, 50 total
        if line.contains("Tests:") && (line.contains("passed") || line.contains("failed")) {
            if let Some(p) = extract_number_before(line, " passed") {
                passed = p;
            }
            if let Some(f) = extract_number_before(line, " failed") {
                failed = f;
            }
        }
        // Jest: Time:        3.2 s
        if line.trim_start().starts_with("Time:") {
            let rest = line.trim_start().trim_start_matches("Time:").trim();
            if let Some(end) = rest.find(" s") {
                duration = Some(format!("{}s", rest[..end].trim()));
            }
        }
        // FAIL src/foo.test.ts
        if line.starts_with("FAIL ") || line.starts_with("  FAIL ") {
            failures.push(format!("`{}`", line.trim().trim_start_matches("FAIL ").trim()));
        }
    }

    let status = if exit_code != 0 || failed > 0 {
        format!("FAILED ({failed} failure{})", if failed == 1 { "" } else { "s" })
    } else {
        "PASSED".to_string()
    };

    TestRunResult {
        status,
        passed,
        failed,
        duration,
        failures,
        raw_output: combined,
    }
}

fn parse_go_test_output(stdout: &str, stderr: &str, exit_code: i32) -> TestRunResult {
    let combined = format!("{stdout}\n{stderr}");
    let mut passed: u32 = 0;
    let mut failed: u32 = 0;
    let mut duration = None;
    let mut failures: Vec<String> = Vec::new();
    let mut total_duration_secs: f64 = 0.0;

    for line in combined.lines() {
        let trimmed = line.trim();
        // ok  \tpackage/name\t0.123s  (tab-separated)
        // ok   package/name  0.123s   (space-separated)
        if trimmed.starts_with("ok") && trimmed.len() > 3
            && trimmed.as_bytes().get(2).map(|b| b.is_ascii_whitespace()).unwrap_or(false)
        {
            passed += 1;
            if let Some(d) = extract_go_duration(trimmed) {
                total_duration_secs += d;
            }
        }
        // FAIL\tpackage/name\t0.456s  (tab-separated)
        // FAIL package/name 0.456s    (space-separated)
        if trimmed.starts_with("FAIL") && trimmed.len() > 5 {
            let rest = trimmed[4..].trim();
            // Skip the bare "FAIL" summary line (no package name)
            if !rest.is_empty() {
                failed += 1;
                let pkg = rest.split_whitespace().next().unwrap_or("");
                if !pkg.is_empty() {
                    failures.push(format!("`{pkg}`"));
                }
                if let Some(d) = extract_go_duration(trimmed) {
                    total_duration_secs += d;
                }
            }
        }
    }

    if total_duration_secs > 0.0 {
        duration = Some(format!("{:.1}s", total_duration_secs));
    }

    let status = if exit_code != 0 || failed > 0 {
        format!("FAILED ({failed} package{})", if failed == 1 { "" } else { "s" })
    } else {
        "PASSED".to_string()
    };

    TestRunResult {
        status,
        passed,
        failed,
        duration,
        failures,
        raw_output: combined,
    }
}

fn parse_generic_output(stdout: &str, stderr: &str, exit_code: i32) -> TestRunResult {
    let combined = format!("{stdout}\n{stderr}");
    let status = if exit_code == 0 { "PASSED" } else { "FAILED" };

    TestRunResult {
        status: status.to_string(),
        passed: 0,
        failed: 0,
        duration: None,
        failures: Vec::new(),
        raw_output: combined,
    }
}

fn extract_number_before(line: &str, suffix: &str) -> Option<u32> {
    if let Some(idx) = line.find(suffix) {
        let before = &line[..idx];
        let num_str: String = before.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<String>().chars().rev().collect();
        num_str.parse().ok()
    } else {
        None
    }
}

fn extract_go_duration(line: &str) -> Option<f64> {
    // Look for something like "0.123s" at the end
    for part in line.split_whitespace().rev() {
        if part.ends_with('s') {
            let num = part.trim_end_matches('s');
            if let Ok(d) = num.parse::<f64>() {
                return Some(d);
            }
        }
    }
    None
}

// --- Gap analysis ---

#[derive(Debug)]
struct TestGap {
    source_path: String,
    suggested_test: String,
}

fn compute_gaps(files: &[ClassifiedFile]) -> Vec<TestGap> {
    let mut gaps = Vec::new();

    // Group source files by directory/module
    let source_files: Vec<&ClassifiedFile> = files
        .iter()
        .filter(|f| f.kind == FileKind::Source && !f.has_inline_tests)
        .collect();

    // Collect test file paths for matching
    let test_paths: Vec<&str> = files
        .iter()
        .filter(|f| f.kind == FileKind::TestFile)
        .map(|f| f.rel_path.as_str())
        .collect();

    for src in &source_files {
        let has_corresponding_test = match src.language.as_str() {
            "rust" => {
                let stem = src.rel_path.trim_end_matches(".rs");
                let basename = src.rel_path.rsplit('/').next().unwrap_or(&src.rel_path).trim_end_matches(".rs");
                test_paths.iter().any(|t| {
                    t.contains(&format!("{basename}_test"))
                        || t.contains(&format!("test_{basename}"))
                        || t.contains(&format!("{basename}_integration"))
                        || *t == format!("tests/{basename}.rs")
                        || *t == format!("tests/{stem}.rs")
                })
            }
            "typescript" | "javascript" => {
                let stem = src.rel_path
                    .trim_end_matches(".ts")
                    .trim_end_matches(".tsx")
                    .trim_end_matches(".js")
                    .trim_end_matches(".jsx");
                test_paths.iter().any(|t| {
                    t.contains(&format!("{stem}.test."))
                        || t.contains(&format!("{stem}.spec."))
                })
            }
            "python" => {
                let basename = src.rel_path.rsplit('/').next().unwrap_or(&src.rel_path).trim_end_matches(".py");
                let dir = src.rel_path.rsplit_once('/').map(|(d, _)| d).unwrap_or(".");
                test_paths.iter().any(|t| {
                    t.contains(&format!("test_{basename}"))
                        || t.contains(&format!("{basename}_test"))
                        || t.contains(&format!("{dir}/tests/test_{basename}"))
                })
            }
            "go" => {
                let test_file = src.rel_path.trim_end_matches(".go");
                test_paths.iter().any(|t| *t == format!("{test_file}_test.go"))
            }
            "java" => {
                let basename = src.rel_path.rsplit('/').next().unwrap_or(&src.rel_path).trim_end_matches(".java");
                test_paths.iter().any(|t| {
                    t.contains(&format!("{basename}Test.java"))
                        || t.contains(&format!("{basename}Tests.java"))
                })
            }
            "php" => {
                let basename = src.rel_path.rsplit('/').next().unwrap_or(&src.rel_path).trim_end_matches(".php");
                test_paths.iter().any(|t| t.contains(&format!("{basename}Test.php")))
            }
            _ => true, // Skip unknown languages
        };

        if !has_corresponding_test {
            let suggested = suggest_test_file(&src.rel_path, &src.language);
            gaps.push(TestGap {
                source_path: src.rel_path.clone(),
                suggested_test: suggested,
            });
        }
    }

    gaps
}

fn suggest_test_file(source_path: &str, language: &str) -> String {
    let basename = source_path.rsplit('/').next().unwrap_or(source_path);
    match language {
        "rust" => {
            let stem = basename.trim_end_matches(".rs");
            format!("tests/{stem}_test.rs or inline #[cfg(test)]")
        }
        "typescript" => {
            let stem = basename.trim_end_matches(".ts").trim_end_matches(".tsx");
            let ext = if basename.ends_with(".tsx") { "tsx" } else { "ts" };
            format!("{stem}.test.{ext}")
        }
        "javascript" => {
            let stem = basename.trim_end_matches(".js").trim_end_matches(".jsx");
            let ext = if basename.ends_with(".jsx") { "jsx" } else { "js" };
            format!("{stem}.test.{ext}")
        }
        "python" => {
            let stem = basename.trim_end_matches(".py");
            format!("test_{stem}.py")
        }
        "go" => {
            let stem = basename.trim_end_matches(".go");
            format!("{stem}_test.go")
        }
        "java" => {
            let stem = basename.trim_end_matches(".java");
            format!("{stem}Test.java")
        }
        "php" => {
            let stem = basename.trim_end_matches(".php");
            format!("{stem}Test.php")
        }
        _ => format!("test for {basename}"),
    }
}

// --- Output formatting ---

fn format_output(
    files: &[ClassifiedFile],
    runner: &Option<DetectedRunner>,
    gaps: &[TestGap],
    test_result: Option<&TestRunResult>,
) -> String {
    let source_count = files.iter().filter(|f| f.kind == FileKind::Source).count();
    let test_count = files.iter().filter(|f| f.kind == FileKind::TestFile).count();
    let inline_count = files.iter().filter(|f| f.kind == FileKind::Source && f.has_inline_tests).count();

    let modules_with_tests = source_count.saturating_sub(gaps.len());
    let coverage_pct = if source_count > 0 {
        (modules_with_tests as f64 / source_count as f64 * 100.0) as u32
    } else {
        100
    };

    let runner_name = runner.as_ref().map(|r| r.name.as_str()).unwrap_or("none detected");

    let mut lines = vec![
        "## Test Adequacy".to_string(),
        String::new(),
        format!("- **Source files:** {source_count}"),
        format!("- **Test files:** {test_count}"),
    ];

    if inline_count > 0 {
        lines.push(format!("- **Inline test modules:** {inline_count} (Rust #[cfg(test)])"));
    }

    lines.push(format!("- **Coverage ratio:** {coverage_pct}% of modules have tests"));
    lines.push(format!("- **Detected runner:** {runner_name}"));

    if !gaps.is_empty() {
        lines.push(String::new());
        lines.push(format!("### Gaps ({} modules with no tests)", gaps.len()));
        lines.push(String::new());
        lines.push("| Module | Suggested Test |".to_string());
        lines.push("|--------|---------------|".to_string());
        for gap in gaps.iter().take(30) {
            lines.push(format!("| `{}` | {} |", gap.source_path, gap.suggested_test));
        }
        if gaps.len() > 30 {
            lines.push(format!("| ... | +{} more |", gaps.len() - 30));
        }
    }

    // Recommendations section: highlight source files in complex directories with no tests
    let recommendations = generate_recommendations(files, gaps);
    if !recommendations.is_empty() {
        lines.push(String::new());
        lines.push("### Recommendations".to_string());
        lines.push(String::new());
        for rec in recommendations.iter().take(10) {
            lines.push(format!("- {rec}"));
        }
    }

    // Test results section (only if tests were run)
    if let Some(result) = test_result {
        lines.push(String::new());
        lines.push("## Test Results".to_string());
        lines.push(String::new());
        lines.push(format!("- **Runner:** {runner_name}"));
        lines.push(format!("- **Status:** {}", result.status));
        if result.passed > 0 || result.failed > 0 {
            lines.push(format!("- **Passed:** {}", result.passed));
            lines.push(format!("- **Failed:** {}", result.failed));
        }
        if let Some(ref d) = result.duration {
            lines.push(format!("- **Duration:** {d}"));
        }

        if !result.failures.is_empty() {
            lines.push(String::new());
            lines.push("### Failures".to_string());
            lines.push(String::new());
            for (i, failure) in result.failures.iter().enumerate().take(20) {
                lines.push(format!("{}. {failure}", i + 1));
            }
            if result.failures.len() > 20 {
                lines.push(format!("\n... and {} more failures", result.failures.len() - 20));
            }
        }

        // If we couldn't parse structured results, show raw output tail
        if result.passed == 0 && result.failed == 0 && result.failures.is_empty() {
            let tail: Vec<&str> = result.raw_output.lines().rev().take(20).collect();
            if !tail.is_empty() {
                lines.push(String::new());
                lines.push("### Output (last 20 lines)".to_string());
                lines.push(String::new());
                lines.push("```".to_string());
                for line in tail.iter().rev() {
                    lines.push(line.to_string());
                }
                lines.push("```".to_string());
            }
        }
    }

    lines.join("\n")
}

fn generate_recommendations(files: &[ClassifiedFile], gaps: &[TestGap]) -> Vec<String> {
    let mut recs = Vec::new();

    // Group gaps by directory
    let mut dir_gaps: HashMap<&str, Vec<&TestGap>> = HashMap::new();
    for gap in gaps {
        let dir = gap.source_path.rsplit_once('/').map(|(d, _)| d).unwrap_or(".");
        dir_gaps.entry(dir).or_default().push(gap);
    }

    // Find directories with many untested files
    let mut sorted_dirs: Vec<(&str, &Vec<&TestGap>)> = dir_gaps.iter().map(|(k, v)| (*k, v)).collect();
    sorted_dirs.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    for (dir, dir_gap_list) in sorted_dirs.iter().take(5) {
        if dir_gap_list.len() >= 2 {
            let file_names: Vec<&str> = dir_gap_list.iter()
                .take(3)
                .map(|g| g.source_path.rsplit('/').next().unwrap_or(&g.source_path))
                .collect();
            let examples = file_names.join(", ");
            recs.push(format!(
                "`{dir}/` — {} source files with no tests (e.g., {examples})",
                dir_gap_list.len()
            ));
        }
    }

    // Highlight individual complex files (those with inline tests should still have integration tests)
    let source_with_inline: Vec<&ClassifiedFile> = files.iter()
        .filter(|f| f.kind == FileKind::Source && f.has_inline_tests)
        .collect();

    if source_with_inline.len() > 3 {
        let count = source_with_inline.len();
        recs.push(format!(
            "{count} files have inline unit tests — consider integration tests in `tests/` for end-to-end validation"
        ));
    }

    recs
}

// --- Main execution ---

pub async fn execute_check_tests(repo_path: &str, run: bool, timeout_secs: u64) -> ToolResult {
    let repo = Path::new(repo_path);
    if !repo.exists() {
        return ToolResult {
            content: format!("Path does not exist: {repo_path}"),
            is_error: true,
        };
    }

    // Walk the repo and classify files
    let mut classified_files: Vec<ClassifiedFile> = Vec::new();
    let mut file_count = 0;

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

        let rel_path = path
            .strip_prefix(repo)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let lang = match language_for_path(&rel_path) {
            Some(l) => l,
            None => continue,
        };

        file_count += 1;
        if file_count > MAX_FILES {
            break;
        }

        // Read file content for inline test detection (Rust #[cfg(test)])
        let content = if lang == "rust" {
            match std::fs::read_to_string(path) {
                Ok(c) => {
                    if c.len() > MAX_FILE_SIZE {
                        String::new()
                    } else {
                        c
                    }
                }
                Err(_) => String::new(),
            }
        } else {
            String::new()
        };

        let classified = classify_file(&rel_path, lang, &content);
        classified_files.push(classified);
    }

    // Detect test runner
    let runner = detect_test_runner(repo);

    // Compute gaps
    let gaps = compute_gaps(&classified_files);

    // Optionally run tests
    let test_result = if run {
        if let Some(ref r) = runner {
            let parts: Vec<&str> = r.command.split_whitespace().collect();
            if parts.is_empty() {
                None
            } else {
                let bin = parts[0];
                let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
                let timeout = Duration::from_secs(timeout_secs);

                // Run the test command IN the repo directory (cwd), not via the
                // unstable `cargo -C` flag (which errors out and was misreported as PASSED).
                let repo = std::path::Path::new(repo_path);
                let exec_result = if bin == "go" {
                    let full_args = vec!["test".to_string(), "./...".to_string()];
                    safe_exec_in_dir(bin, &full_args, repo, timeout).await
                } else {
                    safe_exec_in_dir(bin, &args, repo, timeout).await
                };

                let parsed = match r.name.as_str() {
                    "cargo test" => parse_cargo_test_output(&exec_result.stdout, &exec_result.stderr, exec_result.exit_code),
                    "pytest" => parse_pytest_output(&exec_result.stdout, &exec_result.stderr, exec_result.exit_code),
                    "npm test" | "yarn test" | "pnpm test" => parse_npm_test_output(&exec_result.stdout, &exec_result.stderr, exec_result.exit_code),
                    "go test" => parse_go_test_output(&exec_result.stdout, &exec_result.stderr, exec_result.exit_code),
                    _ => parse_generic_output(&exec_result.stdout, &exec_result.stderr, exec_result.exit_code),
                };

                Some(parsed)
            }
        } else {
            None
        }
    } else {
        None
    };

    let content = format_output(&classified_files, &runner, &gaps, test_result.as_ref());

    ToolResult {
        content,
        is_error: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_rust_source() {
        let f = classify_file("src/tools/blast_radius.rs", "rust", "fn main() {}");
        assert_eq!(f.kind, FileKind::Source);
        assert!(!f.has_inline_tests);
    }

    #[test]
    fn test_classify_rust_test_file() {
        let f = classify_file("tests/integration.rs", "rust", "");
        assert_eq!(f.kind, FileKind::TestFile);
    }

    #[test]
    fn test_classify_rust_inline_tests() {
        let f = classify_file("src/main.rs", "rust", "#[cfg(test)]\nmod tests { #[test] fn it_works() {} }");
        assert_eq!(f.kind, FileKind::Source);
        assert!(f.has_inline_tests);
    }

    #[test]
    fn test_classify_rust_test_suffix() {
        let f = classify_file("src/foo_test.rs", "rust", "");
        assert_eq!(f.kind, FileKind::TestFile);
    }

    #[test]
    fn test_classify_typescript_test() {
        let f = classify_file("src/utils.test.ts", "typescript", "");
        assert_eq!(f.kind, FileKind::TestFile);
    }

    #[test]
    fn test_classify_typescript_spec() {
        let f = classify_file("src/utils.spec.tsx", "typescript", "");
        assert_eq!(f.kind, FileKind::TestFile);
    }

    #[test]
    fn test_classify_typescript_tests_dir() {
        let f = classify_file("src/__tests__/utils.ts", "typescript", "");
        assert_eq!(f.kind, FileKind::TestFile);
    }

    #[test]
    fn test_classify_python_test_prefix() {
        let f = classify_file("tests/test_main.py", "python", "");
        assert_eq!(f.kind, FileKind::TestFile);
    }

    #[test]
    fn test_classify_python_test_suffix() {
        let f = classify_file("src/main_test.py", "python", "");
        assert_eq!(f.kind, FileKind::TestFile);
    }

    #[test]
    fn test_classify_python_tests_dir() {
        let f = classify_file("tests/conftest.py", "python", "");
        assert_eq!(f.kind, FileKind::TestFile);
    }

    #[test]
    fn test_classify_go_test() {
        let f = classify_file("pkg/server_test.go", "go", "");
        assert_eq!(f.kind, FileKind::TestFile);
    }

    #[test]
    fn test_classify_go_source() {
        let f = classify_file("pkg/server.go", "go", "");
        assert_eq!(f.kind, FileKind::Source);
    }

    #[test]
    fn test_classify_java_test() {
        let f = classify_file("src/test/java/AppTest.java", "java", "");
        assert_eq!(f.kind, FileKind::TestFile);
    }

    #[test]
    fn test_classify_java_test_suffix() {
        let f = classify_file("src/main/java/AppTest.java", "java", "");
        assert_eq!(f.kind, FileKind::TestFile);
    }

    #[test]
    fn test_classify_php_test() {
        let f = classify_file("tests/UserTest.php", "php", "");
        assert_eq!(f.kind, FileKind::TestFile);
    }

    #[test]
    fn test_detect_runner_cargo() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
        let runner = detect_test_runner(dir.path()).unwrap();
        assert_eq!(runner.command, "cargo test");
        assert_eq!(runner.name, "cargo test");
    }

    #[test]
    fn test_detect_runner_go() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module example.com/test").unwrap();
        let runner = detect_test_runner(dir.path()).unwrap();
        assert_eq!(runner.command, "go test ./...");
        assert_eq!(runner.name, "go test");
    }

    #[test]
    fn test_detect_runner_npm() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"scripts": {"test": "jest"}}"#).unwrap();
        let runner = detect_test_runner(dir.path()).unwrap();
        assert_eq!(runner.command, "npm test");
    }

    #[test]
    fn test_detect_runner_yarn() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"scripts": {"test": "jest"}}"#).unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();
        let runner = detect_test_runner(dir.path()).unwrap();
        assert_eq!(runner.command, "yarn test");
    }

    #[test]
    fn test_detect_runner_pytest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pytest.ini"), "[pytest]").unwrap();
        let runner = detect_test_runner(dir.path()).unwrap();
        assert_eq!(runner.command, "pytest");
    }

    #[test]
    fn test_detect_runner_gradle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "").unwrap();
        let runner = detect_test_runner(dir.path()).unwrap();
        assert_eq!(runner.command, "./gradlew test");
    }

    #[test]
    fn test_detect_runner_maven() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "").unwrap();
        let runner = detect_test_runner(dir.path()).unwrap();
        assert_eq!(runner.command, "mvn test");
    }

    #[test]
    fn test_extract_number_before() {
        assert_eq!(extract_number_before("test result: ok. 48 passed; 2 failed", " passed"), Some(48));
        assert_eq!(extract_number_before("test result: ok. 48 passed; 2 failed", " failed"), Some(2));
        assert_eq!(extract_number_before("no match here", " passed"), None);
    }

    #[test]
    fn test_parse_cargo_test_pass() {
        let stdout = "running 5 tests\ntest foo ... ok\ntest bar ... ok\ntest result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.2s\n";
        let result = parse_cargo_test_output(stdout, "", 0);
        assert_eq!(result.status, "PASSED");
        assert_eq!(result.passed, 5);
        assert_eq!(result.failed, 0);
        assert_eq!(result.duration, Some("1.2s".to_string()));
    }

    #[test]
    fn test_parse_cargo_test_fail() {
        let stdout = "running 5 tests\ntest foo ... ok\n---- bar stdout ----\nassertion failed: expected true\ntest result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.5s\n";
        let result = parse_cargo_test_output(stdout, "", 101);
        assert_eq!(result.status, "FAILED (1 failure)");
        assert_eq!(result.passed, 4);
        assert_eq!(result.failed, 1);
        assert!(!result.failures.is_empty());
    }

    #[test]
    fn test_parse_cargo_test_runner_errored() {
        // No "test result" line + non-zero exit (e.g. a bad cargo flag or compile error)
        // must NOT be reported as PASSED.
        let stderr = "error: the `-C` flag is unstable, pass `-Z unstable-options`\n";
        let result = parse_cargo_test_output("", stderr, 1);
        assert!(result.status.starts_with("FAILED"), "errored runner must be FAILED, got {:?}", result.status);
        assert_eq!(result.passed, 0);
    }

    #[test]
    fn test_parse_go_test_output() {
        let stdout = "ok  \texample.com/pkg1\t0.5s\nok  \texample.com/pkg2\t0.3s\nFAIL\texample.com/pkg3\t0.2s\n";
        let result = parse_go_test_output(stdout, "", 1);
        assert_eq!(result.passed, 2);
        assert_eq!(result.failed, 1);
    }

    #[test]
    fn test_suggest_test_file_rust() {
        assert_eq!(suggest_test_file("src/main.rs", "rust"), "tests/main_test.rs or inline #[cfg(test)]");
    }

    #[test]
    fn test_suggest_test_file_typescript() {
        assert_eq!(suggest_test_file("src/utils.ts", "typescript"), "utils.test.ts");
    }

    #[test]
    fn test_suggest_test_file_python() {
        assert_eq!(suggest_test_file("src/main.py", "python"), "test_main.py");
    }

    #[test]
    fn test_suggest_test_file_go() {
        assert_eq!(suggest_test_file("pkg/server.go", "go"), "server_test.go");
    }

    #[test]
    fn test_compute_gaps() {
        let files = vec![
            ClassifiedFile {
                rel_path: "src/main.rs".to_string(),
                language: "rust".to_string(),
                kind: FileKind::Source,
                has_inline_tests: false,
            },
            ClassifiedFile {
                rel_path: "src/lib.rs".to_string(),
                language: "rust".to_string(),
                kind: FileKind::Source,
                has_inline_tests: true,
            },
            ClassifiedFile {
                rel_path: "tests/main_test.rs".to_string(),
                language: "rust".to_string(),
                kind: FileKind::TestFile,
                has_inline_tests: false,
            },
        ];
        let gaps = compute_gaps(&files);
        // main.rs has a corresponding test, lib.rs has inline tests -> no gaps
        assert!(gaps.is_empty());
    }

    #[test]
    fn test_compute_gaps_missing() {
        let files = vec![
            ClassifiedFile {
                rel_path: "src/orphan.rs".to_string(),
                language: "rust".to_string(),
                kind: FileKind::Source,
                has_inline_tests: false,
            },
        ];
        let gaps = compute_gaps(&files);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].source_path, "src/orphan.rs");
    }
}
