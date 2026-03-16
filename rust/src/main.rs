mod mcp;
mod runner;
mod shell;
mod tools;
mod types;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use types::{load_embedded_agents, AgentRole};

#[derive(Parser)]
#[command(
    name = "devtribunal",
    version,
    about = "MCP server where each tool is a specialist code review agent"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List all embedded agent definitions
    ListAgents,

    /// Check which recommended linters/tools are installed
    CheckTools,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("DEVTRIBUNAL_LOG_LEVEL")
                .unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::ListAgents) => {
            let agents = load_embedded_agents();
            let mut specialists: Vec<_> = agents
                .values()
                .filter(|a| a.role == AgentRole::Specialist)
                .collect();
            let mut orchestrators: Vec<_> = agents
                .values()
                .filter(|a| a.role == AgentRole::Orchestrator)
                .collect();
            specialists.sort_by_key(|a| &a.name);
            orchestrators.sort_by_key(|a| &a.name);

            println!("{} specialist agents:", specialists.len());
            for a in &specialists {
                let langs = if a.languages.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", a.languages.join(", "))
                };
                println!("  {}{} — {}", a.name, langs, a.description);
            }
            println!();
            println!("{} orchestrator agents:", orchestrators.len());
            for a in &orchestrators {
                println!("  {} — {}", a.name, a.description);
            }
            Ok(())
        }
        Some(Commands::CheckTools) => {
            let agents = load_embedded_agents();
            let result = tools::check_tools::execute_check_tools(&agents).await;
            println!("{}", result.content);
            Ok(())
        }
        None => mcp::serve_stdio().await,
    }
}
