use crate::types::AgentDefinition;
use crate::tools::linter;

pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}

/// Language hint from file extension.
fn lang_for_ext(ext: &str) -> &str {
    match ext {
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "rs" => "rust",
        "go" => "go",
        "java" => "java",
        "php" => "php",
        "cs" => "csharp",
        "c" | "h" => "c",
        "dart" => "dart",
        "lua" => "lua",
        "sql" => "sql",
        "proto" => "protobuf",
        _ => "",
    }
}

fn build_review_prompt(
    agent: &AgentDefinition,
    file_content: &str,
    file_path: &str,
    context: Option<&str>,
    linter_output: Option<&str>,
) -> String {
    let mut parts = Vec::new();

    parts.push(agent.system_prompt.clone());

    if !agent.checklist.is_empty() {
        parts.push("\n## Review Checklist\n".to_string());
        parts.push(agent.checklist.clone());
    }

    parts.push("\n## File Under Review\n".to_string());
    parts.push(format!("**Path:** `{file_path}`\n"));

    let ext = file_path.rsplit('.').next().unwrap_or("");
    let lang = lang_for_ext(ext);
    parts.push(format!("```{lang}"));
    parts.push(file_content.to_string());
    parts.push("```".to_string());

    if let Some(lo) = linter_output {
        if !lo.is_empty() {
            parts.push(lo.to_string());
        }
    }

    if let Some(ctx) = context {
        parts.push("\n## Additional Context\n".to_string());
        parts.push(ctx.to_string());
    }

    // Linter cross-reference note
    if linter_output.is_some_and(|lo| !lo.is_empty()) {
        parts.push("\n## Note\n".to_string());
        parts.push("If linter findings are provided above, reference them where relevant — confirm, expand on, or contextualize what the tools found.".to_string());
    }

    parts.join("\n")
}

pub async fn execute_review(
    agent: &AgentDefinition,
    file_path: &str,
    context: Option<&str>,
) -> ToolResult {
    // Read file
    let file_content = match tokio::fs::read_to_string(file_path).await {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                content: format!(
                    "Cannot read file: {file_path} — {e}. Check that the path is absolute and the file exists."
                ),
                is_error: true,
            };
        }
    };

    // Run linters (best-effort)
    let linter_output = match linter::run_linters(file_path, &agent.recommended_tools).await {
        Ok(result) => {
            let formatted = linter::format_linter_findings(&result);
            if formatted.is_empty() { None } else { Some(formatted) }
        }
        Err(_) => None,
    };

    let prompt = build_review_prompt(
        agent,
        &file_content,
        file_path,
        context,
        linter_output.as_deref(),
    );

    ToolResult {
        content: prompt,
        is_error: false,
    }
}
