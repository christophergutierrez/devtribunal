use std::path::Component;
use std::time::Duration;
use tokio::process::Command;

/// Validate a file path for shell safety.
/// Rejects paths containing characters that could be used for command injection,
/// or that contain `..` path traversal components.
pub fn validate_file_path(file_path: &str) -> Result<(), String> {
    if file_path.contains(|c: char| "`$\"';&|!\n\r\0".contains(c)) {
        return Err(format!(
            "Unsafe file path rejected: path contains shell metacharacters. Path: {}",
            &file_path[..file_path.len().min(200)]
        ));
    }
    let path = std::path::Path::new(file_path);
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
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

/// Split a command template and substitute `{file}` as a single argument.
/// This preserves file paths with spaces as one argument rather than splitting them.
pub fn split_command_with_file(cmd_template: &str, file_path: &str) -> (String, Vec<String>) {
    let parts: Vec<&str> = cmd_template.split_whitespace().collect();
    if parts.is_empty() {
        return (String::new(), Vec::new());
    }
    let bin = parts[0].to_string();
    let args = parts[1..]
        .iter()
        .map(|s| {
            if *s == "{file}" {
                file_path.to_string()
            } else if s.contains("{file}") {
                s.replace("{file}", file_path)
            } else {
                s.to_string()
            }
        })
        .collect();
    (bin, args)
}

#[derive(Debug)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Safe wrapper around tokio::process::Command.
/// Does NOT invoke a shell — arguments are passed directly to the process.
/// On timeout the child process is explicitly killed to avoid zombie processes.
pub async fn safe_exec(
    bin: &str,
    args: &[String],
    timeout: Duration,
) -> ExecResult {
    let mut child = match Command::new(bin)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            return ExecResult {
                stdout: String::new(),
                stderr: format!("Error: spawn {bin}: {e}"),
                exit_code: -1,
            };
        }
    };

    // Take stdout/stderr handles before waiting so we can still kill the child on timeout.
    // wait_with_output() consumes `child`, preventing a later kill() call.
    let mut stdout_handle = child.stdout.take();
    let mut stderr_handle = child.stderr.take();

    tokio::select! {
        status = child.wait() => {
            // Read captured output from the handles
            let stdout = match stdout_handle.take() {
                Some(mut h) => {
                    use tokio::io::AsyncReadExt;
                    let mut buf = Vec::new();
                    let _ = h.read_to_end(&mut buf).await;
                    String::from_utf8_lossy(&buf).to_string()
                }
                None => String::new(),
            };
            let stderr = match stderr_handle.take() {
                Some(mut h) => {
                    use tokio::io::AsyncReadExt;
                    let mut buf = Vec::new();
                    let _ = h.read_to_end(&mut buf).await;
                    String::from_utf8_lossy(&buf).to_string()
                }
                None => String::new(),
            };
            match status {
                Ok(s) => {
                    let code = s.code().unwrap_or(1);
                    ExecResult { stdout, stderr, exit_code: code }
                }
                Err(e) => ExecResult {
                    stdout: String::new(),
                    stderr: format!("Error: waiting on {bin}: {e}"),
                    exit_code: -1,
                },
            }
        }
        _ = tokio::time::sleep(timeout) => {
            // Kill the child process to prevent zombies
            let _ = child.kill().await;
            ExecResult {
                stdout: String::new(),
                stderr: "Process timed out".to_string(),
                exit_code: -1,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_safe_path() {
        assert!(validate_file_path("/home/user/project/src/main.rs").is_ok());
        assert!(validate_file_path("/tmp/test file.ts").is_ok());
        // Filenames containing ".." as a substring (not a path component) are valid
        assert!(validate_file_path("/tmp/foo..bar.ts").is_ok());
        assert!(validate_file_path("/tmp/file...name.rs").is_ok());
    }

    #[test]
    fn test_validate_unsafe_paths() {
        assert!(validate_file_path("/tmp/foo;rm -rf /").is_err());
        assert!(validate_file_path("/tmp/foo`whoami`").is_err());
        assert!(validate_file_path("/tmp/$HOME/foo").is_err());
        assert!(validate_file_path("/tmp/foo'bar").is_err());
        assert!(validate_file_path("/tmp/foo|bar").is_err());
        // Actual path traversal components are still rejected
        assert!(validate_file_path("../../etc/passwd").is_err());
        assert!(validate_file_path("/tmp/../etc/passwd").is_err());
    }

    #[test]
    fn test_split_command() {
        let (bin, args) = split_command("npx eslint --format json /tmp/test.ts");
        assert_eq!(bin, "npx");
        assert_eq!(args, vec!["eslint", "--format", "json", "/tmp/test.ts"]);
    }

    #[test]
    fn test_split_command_with_file_spaces() {
        let (bin, args) = split_command_with_file(
            "npx eslint --format json {file}",
            "/Users/John Smith/project/file.ts",
        );
        assert_eq!(bin, "npx");
        assert_eq!(
            args,
            vec!["eslint", "--format", "json", "/Users/John Smith/project/file.ts"]
        );
    }

    #[test]
    fn test_split_command_with_file_no_placeholder() {
        let (bin, args) = split_command_with_file("golangci-lint run --fix", "/tmp/test.go");
        assert_eq!(bin, "golangci-lint");
        assert_eq!(args, vec!["run", "--fix"]);
    }

    #[test]
    fn test_split_command_with_file_embedded() {
        let (bin, args) = split_command_with_file(
            "ruff check --output-format json {file}",
            "/path with spaces/test.py",
        );
        assert_eq!(bin, "ruff");
        assert_eq!(
            args,
            vec!["check", "--output-format", "json", "/path with spaces/test.py"]
        );
    }
}
