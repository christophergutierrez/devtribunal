use std::time::Duration;
use tokio::process::Command;

/// Validate a file path for shell safety.
/// Rejects paths containing characters that could be used for command injection.
pub fn validate_file_path(file_path: &str) -> Result<(), String> {
    if file_path.contains(|c: char| "`$\"';&|!\n\r\0".contains(c)) {
        return Err(format!(
            "Unsafe file path rejected: path contains shell metacharacters. Path: {}",
            &file_path[..file_path.len().min(200)]
        ));
    }
    if file_path.contains("..") {
        return Err("Unsafe file path rejected: path contains traversal sequence".to_string());
    }
    Ok(())
}

/// Split a command string into binary and arguments.
/// Only safe for known command templates from agent definitions, NOT for arbitrary user input.
pub fn split_command(cmd: &str) -> (String, Vec<String>) {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return (String::new(), Vec::new());
    }
    (
        parts[0].to_string(),
        parts[1..].iter().map(|s| s.to_string()).collect(),
    )
}

#[derive(Debug)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Safe wrapper around tokio::process::Command.
/// Does NOT invoke a shell — arguments are passed directly to the process.
pub async fn safe_exec(
    bin: &str,
    args: &[String],
    timeout: Duration,
) -> ExecResult {
    let result = tokio::time::timeout(timeout, async {
        Command::new(bin)
            .args(args)
            .output()
            .await
    })
    .await;

    match result {
        Ok(Ok(output)) => {
            let code = output.status.code().unwrap_or(1);
            ExecResult {
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                exit_code: code,
            }
        }
        Ok(Err(e)) => ExecResult {
            stdout: String::new(),
            stderr: format!("Error: spawn {bin}: {e}"),
            exit_code: -1,
        },
        Err(_) => ExecResult {
            stdout: String::new(),
            stderr: "Process timed out".to_string(),
            exit_code: -1,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_safe_path() {
        assert!(validate_file_path("/home/user/project/src/main.rs").is_ok());
        assert!(validate_file_path("/tmp/test file.ts").is_ok());
    }

    #[test]
    fn test_validate_unsafe_paths() {
        assert!(validate_file_path("/tmp/foo;rm -rf /").is_err());
        assert!(validate_file_path("/tmp/foo`whoami`").is_err());
        assert!(validate_file_path("/tmp/$HOME/foo").is_err());
        assert!(validate_file_path("/tmp/foo'bar").is_err());
        assert!(validate_file_path("/tmp/foo|bar").is_err());
        assert!(validate_file_path("../../etc/passwd").is_err());
        assert!(validate_file_path("/tmp/../etc/passwd").is_err());
    }

    #[test]
    fn test_split_command() {
        let (bin, args) = split_command("npx eslint --format json /tmp/test.ts");
        assert_eq!(bin, "npx");
        assert_eq!(args, vec!["eslint", "--format", "json", "/tmp/test.ts"]);
    }
}
