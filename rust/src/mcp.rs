//! MCP server over stdin/stdout using JSON-RPC 2.0.

use anyhow::Result;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::backend::{self, BackendConfig, Backend};
use crate::routing::{self, RoutingConfig};
use crate::types::{AgentDefinition, AgentRole, load_embedded_agents, load_agents_from_dir, resolve_agents_dir};

/// Shared server state.
struct ServerState {
    builtin_agents: HashMap<String, AgentDefinition>,
    /// Cache of repo-level agent overrides. Lock must never be held across .await points.
    agent_cache: Mutex<HashMap<PathBuf, HashMap<String, AgentDefinition>>>,
    /// Backend configuration for LLM processing (env-derived global default).
    backend_config: BackendConfig,
    /// Cache of per-repo routing config (`.devtribunal.yml`), keyed by lookup start path.
    /// `None` value = no/invalid config found at that location. Lock never held across .await.
    routing_cache: Mutex<HashMap<PathBuf, Option<RoutingConfig>>>,
}

/// Resolve the effective backend config for `agent_name`, applying per-agent routing from
/// `.devtribunal.yml` (discovered relative to `start_path`) over the env-derived default.
/// Returns the env-derived default unchanged when no routing config applies.
fn effective_backend(
    state: &ServerState,
    agent_name: &str,
    start_path: &str,
    is_directory: bool,
) -> BackendConfig {
    let key = PathBuf::from(start_path);
    let routing = {
        let mut cache = state.routing_cache.lock().unwrap_or_else(|e| {
            tracing::warn!("routing cache mutex was poisoned, recovering");
            e.into_inner()
        });
        cache
            .entry(key)
            .or_insert_with(|| routing::load_routing(start_path, is_directory))
            .clone()
    };
    routing::resolve_backend_for_agent(agent_name, routing.as_ref(), &state.backend_config)
}

/// Run the MCP server over stdio.
pub async fn serve_stdio() -> Result<()> {
    let builtin_agents = load_embedded_agents();
    let backend_config = backend::load_config();

    let specialist_count = builtin_agents.values().filter(|a| a.role == AgentRole::Specialist).count();
    let orchestrator_count = builtin_agents.values().filter(|a| a.role == AgentRole::Orchestrator).count();
    eprintln!(
        "devtribunal v{}\n  {} specialists, {} orchestrators\n  + 10 management tools (dt_init, check_tools, blast_radius, check_tracking, check_deps, check_patterns, check_tests, run_tests, check_secrets, diff_findings)\n  backend: {}",
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
        routing_cache: Mutex::new(HashMap::new()),
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

fn diff_findings_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "previous": { "type": "string", "description": "Previous pass findings as a JSON object {\"findings\":[...]}" },
            "current": { "type": "string", "description": "Current pass findings as a JSON object {\"findings\":[...]}" },
            "previously_fixed": { "type": "array", "items": { "type": "string" }, "description": "Finding ids fixed in earlier passes (for regression detection)" },
            "overrides": { "type": "array", "items": { "type": "object" }, "description": "Architect overrides: [{finding_id, action: escalate|downgrade|dismiss, new_severity?}]" },
            "block_severities": { "type": "array", "items": { "type": "string" }, "description": "Severities that block PASS (default [critical, high])" },
            "max_new": { "type": "integer", "description": "Max new findings allowed for PASS (default 0)" }
        },
        "required": ["previous", "current"]
    })
}

fn check_secrets_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "repo_path": {
                "type": "string",
                "description": "Absolute path to the repository to scan for secrets"
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

    tools.push(json!({
        "name": "check_secrets",
        "description": "Repo-wide secret scan via gitleaks; reports leak locations/rules (values redacted)",
        "inputSchema": check_secrets_input_schema()
    }));

    tools.push(json!({
        "name": "diff_findings",
        "description": "Compare two passes of structured findings (fixed/persisting/new/regressed) and render a PASS/FAIL verdict",
        "inputSchema": diff_findings_input_schema()
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

#[derive(Deserialize)]
struct CheckSecretsInput {
    repo_path: String,
}

#[derive(Deserialize)]
struct DiffFindingsInput {
    previous: String,
    current: String,
    #[serde(default)]
    previously_fixed: Vec<String>,
    #[serde(default)]
    overrides: Vec<crate::tools::diff_findings::Override>,
    block_severities: Option<Vec<String>>,
    max_new: Option<u32>,
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

    if name == "check_secrets" {
        let input: CheckSecretsInput = match serde_json::from_value(args) {
            Ok(v) => v,
            Err(e) => return mcp_error_result(id, &format!("Invalid input: {e}")),
        };
        let result = crate::tools::check_secrets::execute_check_secrets(&input.repo_path).await;
        return mcp_result(id, tool_result(&result.content, result.is_error));
    }

    if name == "diff_findings" {
        let input: DiffFindingsInput = match serde_json::from_value(args) {
            Ok(v) => v,
            Err(e) => return mcp_error_result(id, &format!("Invalid input: {e}")),
        };
        let result = crate::tools::diff_findings::execute_diff_findings(
            &input.previous,
            &input.current,
            &input.previously_fixed,
            &input.overrides,
            input.block_severities.as_deref(),
            input.max_new,
        );
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
            if result.is_error {
                return mcp_result(id, tool_result(&result.content, true));
            }

            // Apply per-agent routing (backend mode only). Orchestrators were previously never
            // sent to the backend; in backend mode the synthesis prompt is now processed too.
            let start = input.repo_path.as_deref().unwrap_or(".");
            let cfg = effective_backend(state, name, start, true);
            let indicator = backend::mode_indicator(&cfg);

            match cfg.backend {
                Backend::Host => {
                    let prefix = cfg
                        .fallback_warning
                        .as_deref()
                        .map(|w| format!("{w}\n\n"))
                        .unwrap_or_default();
                    let content = format!("{prefix}{indicator}\n\n{}", result.content);
                    mcp_result(id, tool_result(&content, false))
                }
                Backend::Api | Backend::Local | Backend::Openai => {
                    match backend::process_review(&cfg, &result.content).await {
                        Ok(Some(out)) => {
                            let content = format!("{indicator}\n\n{out}");
                            mcp_result(id, tool_result(&content, false))
                        }
                        Ok(None) => {
                            let content = format!("{indicator}\n\n{}", result.content);
                            mcp_result(id, tool_result(&content, false))
                        }
                        Err(e) => {
                            let content = format!(
                                "{indicator}\n\nERROR: Backend call failed: {e}\n\nFalling back to raw orchestration prompt:\n\n{}",
                                result.content
                            );
                            mcp_result(id, tool_result(&content, true))
                        }
                    }
                }
            }
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

            // Apply per-agent routing (backend mode only) over the env-derived default.
            let cfg = effective_backend(state, name, &input.file_path, false);
            let indicator = backend::mode_indicator(&cfg);

            match cfg.backend {
                Backend::Host => {
                    // Host mode: return the prompt (linter output + instructions) for the host LLM.
                    // A routing misconfiguration that degraded to host is surfaced as a prefix.
                    let prefix = cfg
                        .fallback_warning
                        .as_deref()
                        .map(|w| format!("{w}\n\n"))
                        .unwrap_or_default();
                    let content = format!("{prefix}{indicator}\n\n{}", result.content);
                    mcp_result(id, tool_result(&content, false))
                }
                Backend::Api | Backend::Local | Backend::Openai => {
                    // Api/Local mode: send the prompt to the backend and return finished findings
                    match backend::process_review(&cfg, &result.content).await {
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

#[cfg(test)]
mod tests {
    use super::{handle_call_tool, handle_list_tools, ServerState};
    use crate::backend;
    use crate::types::load_embedded_agents;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Mutex;

    fn test_state() -> ServerState {
        ServerState {
            builtin_agents: load_embedded_agents(),
            agent_cache: Mutex::new(HashMap::new()),
            backend_config: backend::load_config(),
            routing_cache: Mutex::new(HashMap::new()),
        }
    }

    #[test]
    fn list_tools_includes_all_management_tools() {
        let resp = handle_list_tools(&json!(1), &test_state());
        let names: Vec<String> = resp["result"]["tools"]
            .as_array()
            .expect("tools array")
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        for t in [
            "dt_init", "check_tools", "blast_radius", "check_tracking", "check_deps",
            "check_patterns", "run_tests", "check_secrets", "diff_findings", "check_tests",
        ] {
            assert!(names.contains(&t.to_string()), "tools/list missing {t}");
        }
        assert!(names.iter().any(|n| n == "review_rust"), "specialists should be registered");
    }

    #[tokio::test]
    async fn unknown_tool_returns_error() {
        let params = json!({ "name": "does_not_exist", "arguments": {} });
        let resp = handle_call_tool(&json!(1), &params, &test_state()).await;
        assert_eq!(resp["result"]["isError"].as_bool(), Some(true));
    }

    #[tokio::test]
    async fn malformed_args_return_invalid_input() {
        // check_deps requires repo_path: String; passing a number must be rejected.
        let params = json!({ "name": "check_deps", "arguments": { "repo_path": 123 } });
        let resp = handle_call_tool(&json!(1), &params, &test_state()).await;
        assert_eq!(resp["result"]["isError"].as_bool(), Some(true));
        let text = resp["result"]["content"][0]["text"].as_str().unwrap_or("");
        assert!(text.contains("Invalid input"), "expected 'Invalid input', got: {text}");
    }

    // --- Phase 21: per-agent routing applied in dispatch (AC-4) ---

    #[tokio::test]
    async fn specialist_routing_applied_in_dispatch() {
        // A route whose key_env is unset degrades to host mode with a warning naming the agent —
        // a signal only the routing path produces, proving routing was applied in dispatch.
        std::env::remove_var("ROUTING_TEST_DISPATCH_KEY");
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".devtribunal.yml"),
            "routes:\n  review_rust:\n    provider: anthropic\n    key_env: ROUTING_TEST_DISPATCH_KEY\n",
        )
        .unwrap();
        let file = dir.path().join("foo.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();

        let params = json!({ "name": "review_rust", "arguments": { "file_path": file.to_str().unwrap() } });
        let resp = handle_call_tool(&json!(1), &params, &test_state()).await;
        assert_eq!(resp["result"]["isError"].as_bool(), Some(false));
        let text = resp["result"]["content"][0]["text"].as_str().unwrap_or("");
        assert!(text.contains("degraded to host mode"), "expected routing warning, got: {text}");
        assert!(text.contains("review_rust"), "warning should name the agent, got: {text}");
    }

    #[tokio::test]
    async fn orchestrator_routing_applied_in_dispatch() {
        // Orchestrators were previously never routed; confirm routing now reaches architect.
        std::env::remove_var("ROUTING_TEST_ORCH_KEY");
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".devtribunal.yml"),
            "routes:\n  architect:\n    provider: openai\n    model: grok\n    url: https://api.x.ai/v1\n    key_env: ROUTING_TEST_ORCH_KEY\n",
        )
        .unwrap();

        let params = json!({
            "name": "architect",
            "arguments": { "findings": "## Finding\nspecialist output", "repo_path": dir.path().to_str().unwrap() }
        });
        let resp = handle_call_tool(&json!(1), &params, &test_state()).await;
        assert_eq!(resp["result"]["isError"].as_bool(), Some(false));
        let text = resp["result"]["content"][0]["text"].as_str().unwrap_or("");
        assert!(text.contains("degraded to host mode"), "expected routing warning, got: {text}");
        assert!(text.contains("architect"), "warning should name the agent, got: {text}");
    }

    #[tokio::test]
    async fn orchestrator_without_routing_config_emits_no_routing_warning() {
        // No .devtribunal.yml => env-derived behavior, no routing degradation warning.
        let dir = tempfile::tempdir().unwrap();
        let params = json!({
            "name": "architect",
            "arguments": { "findings": "## Finding\nx", "repo_path": dir.path().to_str().unwrap() }
        });
        let resp = handle_call_tool(&json!(1), &params, &test_state()).await;
        let text = resp["result"]["content"][0]["text"].as_str().unwrap_or("");
        assert!(!text.contains("degraded to host mode"), "no config => no routing warning, got: {text}");
    }
}
