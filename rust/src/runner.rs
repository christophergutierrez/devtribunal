use std::time::Duration;

use crate::shell::{safe_exec, split_command};

const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// Package runner alternatives for cross-ecosystem tool detection.
pub struct RunnerAlternative {
    pub cmd: String,
    pub runner: String,
}

const RUNNER_ALTERNATIVES: &[(&str, &[&str])] = &[
    ("npx", &["bunx", "pnpx", "npx"]),
    ("pip", &["uv pip", "pip", "conda run pip"]),
];

/// Expand a command into alternatives for different package runners.
/// e.g. "npx eslint --version" → ["bunx eslint --version", "pnpx eslint --version", "npx eslint --version"]
/// Commands without a known runner prefix are returned as-is with runner "system".
pub fn expand_runner_alternatives(cmd: &str) -> Vec<RunnerAlternative> {
    for &(prefix, alternatives) in RUNNER_ALTERNATIVES {
        let prefix_space = format!("{prefix} ");
        if cmd.starts_with(&prefix_space) {
            let rest = &cmd[prefix_space.len()..];
            return alternatives
                .iter()
                .map(|alt| RunnerAlternative {
                    cmd: format!("{alt} {rest}"),
                    runner: alt.to_string(),
                })
                .collect();
        }
    }
    vec![RunnerAlternative {
        cmd: cmd.to_string(),
        runner: "system".to_string(),
    }]
}

/// Check if a tool is available by trying its check command with all runner alternatives.
/// Returns the runner that works and version output, or None if not installed.
pub async fn check_tool_available(check_cmd: &str) -> Option<(String, String)> {
    if check_cmd.is_empty() {
        return None;
    }
    let alternatives = expand_runner_alternatives(check_cmd);
    for alt in &alternatives {
        let (bin, args) = split_command(&alt.cmd);
        let result = safe_exec(&bin, &args, CHECK_TIMEOUT).await;
        if result.exit_code == 0 {
            let version = result.stdout.trim().to_string();
            return Some((alt.runner.clone(), version));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_npx_expansion() {
        let alts = expand_runner_alternatives("npx eslint --version");
        assert_eq!(alts.len(), 3);
        assert_eq!(alts[0].cmd, "bunx eslint --version");
        assert_eq!(alts[0].runner, "bunx");
        assert_eq!(alts[1].cmd, "pnpx eslint --version");
        assert_eq!(alts[2].cmd, "npx eslint --version");
    }

    #[test]
    fn test_system_command() {
        let alts = expand_runner_alternatives("golangci-lint --version");
        assert_eq!(alts.len(), 1);
        assert_eq!(alts[0].cmd, "golangci-lint --version");
        assert_eq!(alts[0].runner, "system");
    }

    #[test]
    fn test_pip_expansion() {
        let alts = expand_runner_alternatives("pip install foo");
        assert_eq!(alts.len(), 3);
        assert_eq!(alts[0].runner, "uv pip");
    }
}
