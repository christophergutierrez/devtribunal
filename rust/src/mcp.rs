//! MCP server over stdin/stdout using JSON-RPC 2.0.

use anyhow::Result;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::backend::{self, BackendConfig, Backend};
use crate::types::{AgentDefinition, AgentRole, load_embedded_agents, load_agents_from_dir, resolve_agents_dir};

/// Shared server state.
struct ServerState {
    builtin_agents: HashMap<String, AgentDefinition>,
    /// Cache of repo-level agent overrides. Lock must never be held across .await points.
    agent_cache: Mutex<HashMap<PathBuf, HashMap<String, AgentDefinition>>>,
    /// Backend configuration for LLM processing.
    backend_config: BackendConfig,
}

/// Run the MCP server over stdio.
pub async fn serve_stdio() -> Result<()> {
    let builtin_agents = load_embedded_agents();
    let backend_config = backend::load_config();

    let specialist_count = builtin_agents.values().filter(|a| a.role == AgentRole::Specialist).count();
    let orchestrator_count = builtin_agents.values().filter(|a| a.role == AgentRole::Orchestrator).count();
    eprintln!(
        "devtribunal v{}\n  {} specialists, {} orchestrators\n  + 8 management tools (dt_init, check_tools, blast_radius, check_tracking, check_deps, check_patterns, check_tests, run_tests)\n  backend: {}",
        env!("CARGO_PKG_VERSION"),
        specialist_count,
        orchestrator_count,
        backend::mode_indicator(&backend_config),
    );

    if let Some(ref warning) = backend_config.fallback_warning {
        eprintln!("  {warning}");
    }

    let state = ServerState {
        builtin_agents,
        agent_cache: Mutex::new(HashMap::new()),
        backend_config,
    };

    let stdin = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut lines = stdin.lines();

    tracing::info!("MCP server ready on stdio");

    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {"code": -32700, "message": format!("Parse error: {e}")}
                });
                send(&mut stdout, &resp).await?;
                continue;
            }
        };

        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("");

        let response = match method {
            "initialize" => handle_initialize(&id),
            "initialized" => continue,
            "tools/list" => handle_list_tools(&id, &state),
            "tools/call" => {
                let params = request.get("params").cloned().unwrap_or(json!({}));
                handle_call_tool(&id, &params, &state).await
            }
            "notifications/cancelled" | "notifications/initialized" => continue,
            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("Method not found: {method}")}
            }),
        };

        send(&mut stdout, &response).await?;
    }

    Ok(())
}

async fn send(stdout: &mut tokio::io::Stdout, msg: &Value) -> Result<()> {
    let s = serde_json::to_string(msg)?;
    stdout.write_all(s.as_bytes()).await?;
    stdout.write_all(b"\n").await?;
    stdout.flush().await?;
    Ok(())
}

fn handle_initialize(id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "devtribunal",
                "version": env!("CARGO_PKG_VERSION")
            }
        }
    })
}

// --- Input schemas for MCP tool definitions ---

fn review_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "file_path": {
                "type": "string",
                "description": "Absolute path to the file to review"
            },
            "context": {
                "type": "string",
                "description": "Additional context about the file or review focus"
            }
        },
        "required": ["file_path"]
    })
}

fn orchestrate_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "findings": {
                "type": "string",
                "description": "Specialist review findings (structured Markdown from specialist agents)"
            },
            "context": {
                "type": "string",
                "description": "Additional context about the review scope or priorities"
            },
            "repo_path": {
                "type": "string",
                "description": "Absolute path to the repository (used to resolve agent overrides from .devtribunal_agents/)"
            }
        },
        "required": ["findings"]
    })
}

fn init_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "repo_path": {
                "type": "string",
                "description": "Absolute path to the target repository"
            },
            "languages": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Languages to initialize agents for (auto-detected if omitted)"
            }
        },
        "required": ["repo_path"]
    })
}

fn check_tools_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "repo_path": {
                "type": "string",
                "description": "Absolute path to the repo (uses .devtribunal_agents/ if present)"
            }
        },
        "required": []
    })
}

fn blast_radius_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "repo_path": {
                "type": "string",
                "description": "Absolute path to the git repository"
            },
            "scope": {
                "type": "string",
                "description": "Diff scope: \"staged\", \"unpushed\", or a git ref range like \"main..HEAD\""
            }
        },
        "required": ["repo_path", "scope"]
    })
}

fn check_tracking_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "repo_path": {
                "type": "string",
                "description": "Absolute path to the git repository"
            }
        },
        "required": ["repo_path"]
    })
}

fn check_deps_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "repo_path": {
                "type": "string",
                "description": "Absolute path to the repository containing lockfiles"
            }
        },
        "required": ["repo_path"]
    })
}

fn check_patterns_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "repo_path": {
                "type": "string",
                "description": "Absolute path to the repository to analyze"
            },
            "languages": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Languages to analyze (e.g. [\"rust\", \"typescript\"]). Auto-detected if omitted."
            }
        },
        "required": ["repo_path"]
    })
}

fn check_tests_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "repo_path": {
                "type": "string",
                "description": "Absolute path to the repository to analyze"
            },
            "run": {
                "type": "boolean",
                "description": "Whether to execute the detected test runner (default: false)"
            },
            "timeout_secs": {
                "type": "integer",
                "description": "Timeout in seconds for test execution (default: 120)"
            }
        },
        "required": ["repo_path"]
    })
}

fn run_tests_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "repo_path": {
                "type": "string",
                "description": "Absolute path to the repository to run tests in"
            },
            "test_command": {
                "type": "string",
                "description": "Override the auto-detected test command (e.g. \"cargo test\", \"pytest -q\")"
            },
            "timeout_secs": {
                "type": "integer",
                "description": "Timeout in seconds (default 300)"
            }
        },
        "required": ["repo_path"]
    })
}

fn handle_list_tools(id: &Value, state: &ServerState) -> Value {
    let mut tools = Vec::new();

    for agent in state.builtin_agents.values() {
        match agent.role {
            AgentRole::Specialist => {
                tools.push(json!({
                    "name": agent.name,
                    "description": agent.description,
                    "inputSchema": review_input_schema()
                }));
            }
            AgentRole::Orchestrator => {
                tools.push(json!({
                    "name": agent.name,
                    "description": agent.description,
                    "inputSchema": orchestrate_input_schema()
                }));
            }
        }
    }

    // Management tools
    tools.push(json!({
        "name": "dt_init",
        "description": "Initialize devtribunal in a target repo — scaffolds agent definitions and Claude Code skill commands",
        "inputSchema": init_input_schema()
    }));

    tools.push(json!({
        "name": "check_tools",
        "description": "Check which recommended linters/tools are installed for the loaded agents",
        "inputSchema": check_tools_input_schema()
    }));

    tools.push(json!({
        "name": "blast_radius",
        "description": "Diff-aware impact analysis: identifies changed symbols and files that depend on them",
        "inputSchema": blast_radius_input_schema()
    }));

    tools.push(json!({
        "name": "check_tracking",
        "description": "Git hygiene audit: finds tracked secrets/artifacts and ignored source files, with fix commands",
        "inputSchema": check_tracking_input_schema()
    }));

    tools.push(json!({
        "name": "check_deps",
        "description": "Dependency vulnerability audit: queries OSV.dev for known CVEs in lockfile dependencies",
        "inputSchema": check_deps_input_schema()
    }));

    tools.push(json!({
        "name": "check_patterns",
        "description": "Cross-file structural analysis: circular dependencies, dead exports, duplicated literals, error inconsistencies",
        "inputSchema": check_patterns_input_schema()
    }));

    tools.push(json!({
        "name": "check_tests",
        "description": "Test adequacy analysis: detects test coverage gaps, identifies test runner, and optionally executes tests",
        "inputSchema": check_tests_input_schema()
    }));

    tools.push(json!({
        "name": "run_tests",
        "description": "Detect and run the repo's test suite; returns pass/fail summary (verification signal for the convergence loop)",
        "inputSchema": run_tests_input_schema()
    }));

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": { "tools": tools }
    })
}

// --- Input deserialization ---

#[derive(Deserialize)]
struct ReviewInput {
    file_path: String,
    context: Option<String>,
}

#[derive(Deserialize)]
struct OrchestrateInput {
    findings: String,
    context: Option<String>,
    repo_path: Option<String>,
}

#[derive(Deserialize)]
struct InitInput {
    repo_path: String,
    languages: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct CheckToolsInput {
    repo_path: Option<String>,
}

#[derive(Deserialize)]
struct BlastRadiusInput {
    repo_path: String,
    scope: String,
}

#[derive(Deserialize)]
struct CheckTrackingInput {
    repo_path: String,
}

#[derive(Deserialize)]
struct CheckDepsInput {
    repo_path: String,
}

#[derive(Deserialize)]
struct CheckPatternsInput {
    repo_path: String,
    languages: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct CheckTestsInput {
    repo_path: String,
    run: Option<bool>,
    timeout_secs: Option<u64>,
}

#[derive(Deserialize)]
struct RunTestsInput {
    repo_path: String,
    test_command: Option<String>,
    timeout_secs: Option<u64>,
}

fn tool_result(text: &str, is_error: bool) -> Value {
    json!({
        "content": [{"type": "text", "text": text}],
        "isError": is_error
    })
}

fn mcp_result(id: &Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn mcp_error_result(id: &Value, text: &str) -> Value {
    mcp_result(id, tool_result(text, true))
}

/// Resolve the agent to use for a review: repo-specific override > builtin.
fn resolve_agent(
    name: &str,
    file_path: &str,
    state: &ServerState,
) -> Option<AgentDefinition> {
    // Check for repo-level .devtribunal_agents/
    if let Some(agents_dir) = resolve_agents_dir(file_path, false) {
        let mut cache = state.agent_cache.lock().unwrap_or_else(|e| {
            tracing::warn!("agent cache mutex was poisoned, recovering");
            e.into_inner()
        });
        let repo_agents = cache.entry(agents_dir.clone()).or_insert_with(|| {
            load_agents_from_dir(&agents_dir).unwrap_or_default()
        });
        if let Some(agent) = repo_agents.get(name) {
            return Some(agent.clone());
        }
    }
    // Fall back to builtin
    state.builtin_agents.get(name).cloned()
}

async fn handle_call_tool(id: &Value, params: &Value, state: &ServerState) -> Value {
    let name = params
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    // Management tools
    if name == "dt_init" {
        let input: InitInput = match serde_json::from_value(args) {
            Ok(v) => v,
            Err(e) => return mcp_error_result(id, &format!("Invalid input: {e}")),
        };
        let result = crate::tools::init::execute_init(&input.repo_path, input.languages.as_deref());
        return mcp_result(id, tool_result(&result.content, result.is_error));
    }

    if name == "check_tools" {
        let input: CheckToolsInput = match serde_json::from_value(args) {
            Ok(v) => v,
            Err(e) => return mcp_error_result(id, &format!("Invalid input: {e}")),
        };
        let agents = if let Some(repo_path) = &input.repo_path {
            if let Some(agents_dir) = resolve_agents_dir(repo_path, true) {
                let mut cache = state.agent_cache.lock().unwrap_or_else(|e| {
                    tracing::warn!("agent cache mutex was poisoned, recovering");
                    e.into_inner()
                });
                cache.entry(agents_dir.clone()).or_insert_with(|| {
                    load_agents_from_dir(&agents_dir).unwrap_or_default()
                }).clone()
            } else {
                state.builtin_agents.clone()
            }
        } else {
            state.builtin_agents.clone()
        };
        let result = crate::tools::check_tools::execute_check_tools(&agents).await;
        return mcp_result(id, tool_result(&result.content, result.is_error));
    }

    if name == "blast_radius" {
        let input: BlastRadiusInput = match serde_json::from_value(args) {
            Ok(v) => v,
            Err(e) => return mcp_error_result(id, &format!("Invalid input: {e}")),
        };
        let result = crate::tools::blast_radius::execute_blast_radius(&input.repo_path, &input.scope).await;
        return mcp_result(id, tool_result(&result.content, result.is_error));
    }

    if name == "check_tracking" {
        let input: CheckTrackingInput = match serde_json::from_value(args) {
            Ok(v) => v,
            Err(e) => return mcp_error_result(id, &format!("Invalid input: {e}")),
        };
        let result = crate::tools::check_tracking::execute_check_tracking(&input.repo_path).await;
        return mcp_result(id, tool_result(&result.content, result.is_error));
    }

    if name == "check_deps" {
        let input: CheckDepsInput = match serde_json::from_value(args) {
            Ok(v) => v,
            Err(e) => return mcp_error_result(id, &format!("Invalid input: {e}")),
        };
        let result = crate::tools::check_deps::execute_check_deps(&input.repo_path).await;
        return mcp_result(id, tool_result(&result.content, result.is_error));
    }

    if name == "check_patterns" {
        let input: CheckPatternsInput = match serde_json::from_value(args) {
            Ok(v) => v,
            Err(e) => return mcp_error_result(id, &format!("Invalid input: {e}")),
        };
        let result = crate::tools::check_patterns::execute_check_patterns(&input.repo_path, input.languages.as_deref()).await;
        return mcp_result(id, tool_result(&result.content, result.is_error));
    }

    if name == "check_tests" {
        let input: CheckTestsInput = match serde_json::from_value(args) {
            Ok(v) => v,
            Err(e) => return mcp_error_result(id, &format!("Invalid input: {e}")),
        };
        let run = input.run.unwrap_or(false);
        let timeout_secs = input.timeout_secs.unwrap_or(120);
        let result = crate::tools::check_tests::execute_check_tests(&input.repo_path, run, timeout_secs).await;
        return mcp_result(id, tool_result(&result.content, result.is_error));
    }

    if name == "run_tests" {
        let input: RunTestsInput = match serde_json::from_value(args) {
            Ok(v) => v,
            Err(e) => return mcp_error_result(id, &format!("Invalid input: {e}")),
        };
        let result = crate::tools::run_tests::execute_run_tests(
            &input.repo_path,
            input.test_command.as_deref(),
            input.timeout_secs,
        )
        .await;
        return mcp_result(id, tool_result(&result.content, result.is_error));
    }

    // Look up agent
    let builtin_agent = match state.builtin_agents.get(name) {
        Some(a) => a,
        None => return mcp_error_result(id, &format!("Unknown tool: {name}")),
    };

    match builtin_agent.role {
        AgentRole::Orchestrator => {
            let input: OrchestrateInput = match serde_json::from_value(args) {
                Ok(v) => v,
                Err(e) => return mcp_error_result(id, &format!("Invalid input: {e}")),
            };
            let agent = if let Some(ref repo_path) = input.repo_path {
                resolve_agent(name, repo_path, state)
                    .unwrap_or_else(|| builtin_agent.clone())
            } else {
                builtin_agent.clone()
            };
            let result = crate::tools::orchestrate::execute_orchestrate(&agent, &input.findings, input.context.as_deref());
            mcp_result(id, tool_result(&result.content, result.is_error))
        }
        AgentRole::Specialist => {
            let input: ReviewInput = match serde_json::from_value(args) {
                Ok(v) => v,
                Err(e) => return mcp_error_result(id, &format!("Invalid input: {e}")),
            };
            let agent = resolve_agent(name, &input.file_path, state)
                .unwrap_or_else(|| builtin_agent.clone());
            let result = crate::tools::review::execute_review(&agent, &input.file_path, input.context.as_deref()).await;

            if result.is_error {
                return mcp_result(id, tool_result(&result.content, true));
            }

            let indicator = backend::mode_indicator(&state.backend_config);

            match state.backend_config.backend {
                Backend::Host => {
                    // Host mode: return the prompt (linter output + instructions) for the host LLM
                    let content = format!("{indicator}\n\n{}", result.content);
                    mcp_result(id, tool_result(&content, false))
                }
                Backend::Api | Backend::Local => {
                    // Api/Local mode: send the prompt to the backend and return finished findings
                    match backend::process_review(&state.backend_config, &result.content).await {
                        Ok(Some(findings)) => {
                            let content = format!("{indicator}\n\n{findings}");
                            mcp_result(id, tool_result(&content, false))
                        }
                        Ok(None) => {
                            // Should not happen for Api/Local, but handle gracefully
                            let content = format!("{indicator}\n\n{}", result.content);
                            mcp_result(id, tool_result(&content, false))
                        }
                        Err(e) => {
                            let content = format!(
                                "{indicator}\n\nERROR: Backend call failed: {e}\n\nFalling back to raw review prompt:\n\n{}",
                                result.content
                            );
                            mcp_result(id, tool_result(&content, true))
                        }
                    }
                }
            }
        }
    }
}
