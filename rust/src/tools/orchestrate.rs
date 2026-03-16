use crate::types::AgentDefinition;
use crate::tools::review::ToolResult;

fn build_orchestrate_prompt(
    agent: &AgentDefinition,
    findings: &str,
    context: Option<&str>,
) -> String {
    let mut parts = Vec::new();

    parts.push(agent.system_prompt.clone());

    if !agent.checklist.is_empty() {
        parts.push("\n## Process\n".to_string());
        parts.push(agent.checklist.clone());
    }

    parts.push("\n## Specialist Findings\n".to_string());
    parts.push(findings.to_string());

    if let Some(ctx) = context {
        parts.push("\n## Additional Context\n".to_string());
        parts.push(ctx.to_string());
    }

    if !agent.output_format.is_empty() {
        parts.push("\n## Required Output Format\n".to_string());
        parts.push(agent.output_format.clone());
    }

    parts.join("\n")
}

pub fn execute_orchestrate(
    agent: &AgentDefinition,
    findings: &str,
    context: Option<&str>,
) -> ToolResult {
    if findings.trim().is_empty() {
        return ToolResult {
            content: "Empty findings string. Expected structured Markdown from specialist agent output.".to_string(),
            is_error: true,
        };
    }

    let prompt = build_orchestrate_prompt(agent, findings, context);
    ToolResult {
        content: prompt,
        is_error: false,
    }
}
