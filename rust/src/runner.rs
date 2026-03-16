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
