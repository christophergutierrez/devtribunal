use std::collections::{HashMap, HashSet};

use crate::runner::check_tool_available;
use crate::types::AgentDefinition;
use super::ToolResult;

struct ToolCheckResult {
    tool: String,
    purpose: String,
    installed: bool,
    version: String,
    runner: String,
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
                let result = check_tool_available(&check_cmd).await;
                ToolCheckResult {
                    tool: tool_name,
                    purpose: tool_purpose,
                    installed: result.is_some(),
                    runner: result.as_ref().map(|(r, _)| r.clone()).unwrap_or_default(),
                    version: result.map(|(_, v)| v).unwrap_or_default(),
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

    tool_results.sort_by(|a, b| a.tool.cmp(&b.tool));

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
