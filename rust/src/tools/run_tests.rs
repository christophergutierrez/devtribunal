//! `run_tests` — detect and run the target repo's test suite, returning a
//! machine-consumable pass/fail summary. The convergence loop's primary
//! verification signal. Exit code is authoritative for `ok`; counts are
//! best-effort. A failing suite is data, not a tool error (is_error stays false).

use std::path::Path;
use std::time::Duration;

use regex::Regex;
use serde_json::json;

use super::ToolResult;
use crate::shell::{safe_exec_in_dir, split_command, ExecResult};

const DEFAULT_TIMEOUT_SECS: u64 = 300;

struct DetectedTests {
    framework: String,
    command: String,
}

/// Map common project markers to a test command.
fn detect_test_command(repo: &Path) -> Option<DetectedTests> {
    let has = |p: &str| repo.join(p).exists();
    let make = |fw: &str, cmd: &str| {
        Some(DetectedTests {
            framework: fw.to_string(),
            command: cmd.to_string(),
        })
    };

    if has("Cargo.toml") {
        return make("cargo", "cargo test");
    }
    if has("go.mod") {
        return make("go", "go test ./...");
    }
    if has("pyproject.toml") || has("pytest.ini") || has("setup.cfg") || has("tox.ini") {
        return make("pytest", "pytest");
    }
    if has("package.json") {
        let cmd = if has("bun.lockb") {
            "bun test"
        } else if has("pnpm-lock.yaml") {
            "pnpm test"
        } else {
            "npm test"
        };
        return make("node", cmd);
    }
    None
}

struct TestSummary {
    passed: Option<u32>,
    failed: Option<u32>,
    total: Option<u32>,
    ok: bool,
    timed_out: bool,
    exit_code: i32,
}

/// Best-effort count extraction. cargo ("5 passed; 0 failed"), pytest ("5 passed"),
/// and jest ("5 passed, 6 total") all surface "<n> passed"/"<n> failed" tokens, so a
/// single generic scan (summed across occurrences) covers them.
fn parse_counts(text: &str) -> (Option<u32>, Option<u32>) {
    let passed_re = Regex::new(r"(\d+)\s+passed").expect("valid regex");
    let failed_re = Regex::new(r"(\d+)\s+failed").expect("valid regex");
    let sum = |re: &Regex| -> Option<u32> {
        if !re.is_match(text) {
            return None;
        }
        Some(
            re.captures_iter(text)
                .filter_map(|c| c[1].parse::<u32>().ok())
                .sum(),
        )
    };
    (sum(&passed_re), sum(&failed_re))
}

fn parse_test_output(_framework: &str, r: &ExecResult, timed_out: bool) -> TestSummary {
    let text = format!("{}\n{}", r.stdout, r.stderr);
    let (passed, failed) = parse_counts(&text);
    let total = match (passed, failed) {
        (Some(p), Some(f)) => Some(p + f),
        _ => None,
    };
    TestSummary {
        passed,
        failed,
        total,
        ok: !timed_out && r.exit_code == 0,
        timed_out,
        exit_code: r.exit_code,
    }
}

pub async fn execute_run_tests(
    repo_path: &str,
    test_command: Option<&str>,
    timeout_secs: Option<u64>,
) -> ToolResult {
    let repo = Path::new(repo_path);
    if !repo.is_dir() {
        return ToolResult {
            content: format!("repo_path is not a directory: {repo_path}"),
            is_error: true,
        };
    }

    let (framework, command) = match test_command {
        Some(c) if !c.trim().is_empty() => ("override".to_string(), c.trim().to_string()),
        _ => match detect_test_command(repo) {
            Some(d) => (d.framework, d.command),
            None => {
                let body = json!({ "ok": null, "reason": "no test command detected" });
                return ToolResult {
                    content: format!(
                        "No test command detected for {repo_path}. Pass test_command to run a suite explicitly.\n\n```json\n{}\n```",
                        serde_json::to_string_pretty(&body).unwrap_or_default()
                    ),
                    is_error: false,
                };
            }
        },
    };

    let timeout = Duration::from_secs(timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS));
    let (bin, args) = split_command(&command);
    let result = safe_exec_in_dir(&bin, &args, repo, timeout).await;
    let timed_out = result.exit_code == -1 && result.stderr.contains("timed out");
    let summary = parse_test_output(&framework, &result, timed_out);

    let body = json!({
        "framework": framework,
        "command": command,
        "ok": summary.ok,
        "passed": summary.passed,
        "failed": summary.failed,
        "total": summary.total,
        "exit_code": summary.exit_code,
        "timed_out": summary.timed_out,
    });
    let status = if summary.ok {
        "PASS"
    } else if timed_out {
        "TIMED OUT"
    } else {
        "FAIL"
    };
    let fmt = |n: Option<u32>| n.map(|v| v.to_string()).unwrap_or_else(|| "?".to_string());
    let content = format!(
        "## Test Run\n\n- **Command:** `{command}`\n- **Result:** {status}\n- **Passed:** {}\n- **Failed:** {}\n- **Exit code:** {}\n\n```json\n{}\n```",
        fmt(summary.passed),
        fmt(summary.failed),
        summary.exit_code,
        serde_json::to_string_pretty(&body).unwrap_or_default(),
    );

    ToolResult {
        content,
        is_error: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_repo(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dt_rt_{}_{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detects_by_marker() {
        let cargo = temp_repo("cargo");
        std::fs::write(cargo.join("Cargo.toml"), "[package]").unwrap();
        assert_eq!(detect_test_command(&cargo).unwrap().command, "cargo test");

        let go = temp_repo("go");
        std::fs::write(go.join("go.mod"), "module x").unwrap();
        assert_eq!(detect_test_command(&go).unwrap().command, "go test ./...");

        let bun = temp_repo("bun");
        std::fs::write(bun.join("package.json"), "{}").unwrap();
        std::fs::write(bun.join("bun.lockb"), "").unwrap();
        assert_eq!(detect_test_command(&bun).unwrap().command, "bun test");

        let empty = temp_repo("empty");
        assert!(detect_test_command(&empty).is_none());

        for d in [cargo, go, bun, empty] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn parses_counts_across_frameworks() {
        assert_eq!(parse_counts("test result: ok. 5 passed; 0 failed; 0 ignored"), (Some(5), Some(0)));
        assert_eq!(parse_counts("===== 3 failed, 2 passed in 0.1s ====="), (Some(2), Some(3)));
        assert_eq!(parse_counts("Tests: 1 failed, 5 passed, 6 total"), (Some(5), Some(1)));
        assert_eq!(parse_counts("no counts here"), (None, None));
    }

    #[tokio::test]
    async fn passing_and_failing_commands_map_to_ok() {
        let repo = temp_repo("exec");
        let pass = execute_run_tests(repo.to_str().unwrap(), Some("true"), Some(10)).await;
        assert!(!pass.is_error);
        assert!(pass.content.contains("\"ok\": true"));

        let fail = execute_run_tests(repo.to_str().unwrap(), Some("false"), Some(10)).await;
        assert!(!fail.is_error);
        assert!(fail.content.contains("\"ok\": false"));
        let _ = std::fs::remove_dir_all(repo);
    }

    #[tokio::test]
    async fn no_command_degrades_gracefully() {
        let repo = temp_repo("nocmd");
        let res = execute_run_tests(repo.to_str().unwrap(), None, None).await;
        assert!(!res.is_error);
        assert!(res.content.contains("no test command detected"));
        let _ = std::fs::remove_dir_all(repo);
    }
}
