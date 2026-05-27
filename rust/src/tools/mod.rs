pub mod blast_radius;
pub mod check_deps;
pub mod check_patterns;
pub mod check_secrets;
pub mod check_tools;
pub mod check_tracking;
pub mod diff_findings;
pub mod init;
pub mod linter;
pub mod orchestrate;
pub mod review;
pub mod run_tests;

pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}
