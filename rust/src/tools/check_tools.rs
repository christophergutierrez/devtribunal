use std::collections::{HashMap, HashSet};

use crate::runner::expand_runner_alternatives;
use crate::shell::{safe_exec, split_command};
use crate::tools::review::ToolResult;
use crate::types::AgentDefinition;

use std::time::Duration;

const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

struct ToolCheckResult {
    tool: String,
    purpose: String,
    installed: bool,
    version: String,
    runner: String,
}

async fn check_command(cmd: &str) -> Option<(String, String)> {
    let alternatives = expand_runner_alternatives(cmd);
    for alt in &alternatives {
        let (bin, args) = split_command(&alt.cmd);
        let result = safe_exec(&bin, &args, CHECK_TIMEOUT).await;
        if result.exit_code == 0 && !result.stdout.trim().is_empty() {
            return Some((result.stdout.trim().to_string(), alt.runner.clone()));
        }
    }
    None
}

pub async fn execute_check_tools(agents: &HashMap<String, AgentDefinition>) -> ToolResult {
    let mut handles = Vec::new();
    let mut seen = HashSet::new();

    for agent in agents.values() {
        for tool in &agent.recommended_tools {
            if tool.check.is_empty() || !seen.insert(tool.name.clone()) {
                continue;
            }
            let tool_name = tool.name.clone();
            let tool_purpose = tool.purpose.clone();
            let check_cmd = tool.check.clone();

            handles.push(tokio::spawn(async move {
                let result = check_command(&check_cmd).await;
                ToolCheckResult {
                    tool: tool_name,
                    purpose: tool_purpose,
                    installed: result.is_some(),
                    version: result.as_ref().map(|(v, _)| v.clone()).unwrap_or_default(),
                    runner: result.map(|(_, r)| r).unwrap_or_default(),
                }
            }));
        }
    }

    let mut tool_results = Vec::new();
    for handle in handles {
        if let Ok(result) = handle.await {
            tool_results.push(result);
        }
    }

    if tool_results.is_empty() {
        return ToolResult {
            content: "No recommended tools defined in loaded agents.".to_string(),
            is_error: false,
        };
    }

    let installed: Vec<&ToolCheckResult> = tool_results.iter().filter(|r| r.installed).collect();
    let missing: Vec<&ToolCheckResult> = tool_results.iter().filter(|r| !r.installed).collect();

    let mut lines = vec![
        format!(
            "Tool availability: {} of {} recommended tools found",
            installed.len(),
            tool_results.len()
        ),
        String::new(),
    ];

    if !installed.is_empty() {
        lines.push("Installed:".to_string());
        for r in &installed {
            let via = if !r.runner.is_empty() && r.runner != "system" {
                format!(" [via {}]", r.runner)
            } else {
                String::new()
            };
            let version = if !r.version.is_empty() {
                format!(" — {}", r.version)
            } else {
                String::new()
            };
            lines.push(format!("  ✓ {} ({}){}{}", r.tool, r.purpose, via, version));
        }
    }

    if !missing.is_empty() {
        lines.push(String::new());
        lines.push("Missing (optional but recommended):".to_string());
        for r in &missing {
            lines.push(format!("  ✗ {} — {}", r.tool, r.purpose));
        }
        lines.push(String::new());
        lines.push(
            "Reviews work without these tools, but findings will be more comprehensive with them."
                .to_string(),
        );
    }

    ToolResult {
        content: lines.join("\n"),
        is_error: false,
    }
}
