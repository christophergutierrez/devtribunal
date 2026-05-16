use crate::lang::language_for_path;
use crate::types::AgentDefinition;
use crate::tools::linter;

pub use super::ToolResult;

const MAX_FILE_SIZE: usize = 512 * 1024; // 512KB

pub async fn execute_review(
    agent: &AgentDefinition,
    file_path: &str,
    _context: Option<&str>,
) -> ToolResult {
    // Check file exists and get metadata
    let metadata = match tokio::fs::metadata(file_path).await {
        Ok(m) => m,
        Err(e) => {
            return ToolResult {
                content: format!(
                    "Cannot read file: {file_path} — {e}. Check that the path is absolute and the file exists."
                ),
                is_error: true,
            };
        }
    };

    let file_size = metadata.len() as usize;

    if file_size > MAX_FILE_SIZE {
        let size_kb = file_size / 1024;
        return ToolResult {
            content: format!(
                "File skipped: {file_path} is {size_kb}KB (limit: {}KB). Likely generated or minified — exclude from review.",
                MAX_FILE_SIZE / 1024
            ),
            is_error: false,
        };
    }

    // Run linters (best-effort)
    let linter_section = match linter::run_linters(file_path, &agent.recommended_tools).await {
        Ok(result) => {
            let formatted = linter::format_linter_findings(&result);
            if formatted.is_empty() {
                "No linter output available".to_string()
            } else {
                formatted
            }
        }
        Err(e) => {
            tracing::warn!("linter execution failed: {e}");
            format!("Linter execution failed: {e}")
        }
    };

    let language = language_for_path(file_path).unwrap_or("unknown");

    let output = format!(
        "## Linter Output\n\n{linter_section}\n\n## File Metadata\n\n- **Path:** {file_path}\n- **Language:** {language}\n- **Size:** {file_size} bytes\n- **Agent:** {}",
        agent.name
    );

    ToolResult {
        content: output,
        is_error: false,
    }
}
