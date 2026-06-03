//! Per-agent model routing (backend mode only).
//!
//! A version-controlled `.devtribunal.yml` at (or above) the repo root maps each agent
//! — specialist or orchestrator — to a `{provider, model, url, key_env}` route, with a
//! `default` fallback. At dispatch the server resolves the effective [`BackendConfig`]
//! per agent so different specialists can target different models within one repo.
//!
//! Routing is a **backend-mode** feature. In host mode the host picks the model, so a
//! present `.devtribunal.yml` is informational only.
//!
//! Precedence: `routes[agent]` > `default` > env-derived global config (Phase 20 behavior).
//! When no `.devtribunal.yml` exists, resolution returns the env-derived config unchanged,
//! preserving full backward compatibility.

use std::collections::HashMap;

use serde::Deserialize;

use crate::backend::{Backend, BackendConfig};

/// Provider vocabulary exposed in `.devtribunal.yml`.
///
/// `openai` covers any OpenAI-compatible endpoint: keyed (OpenAI/xAI/OpenRouter, via
/// `key_env`) or keyless (local Ollama/vLLM/llama.cpp, `key_env` omitted).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Host,
    Anthropic,
    Openai,
}

/// A single route entry.
#[derive(Debug, Clone, Deserialize)]
pub struct RouteConfig {
    pub provider: Provider,
    #[serde(default)]
    pub model: Option<String>,
    /// Base URL — required for `openai`, ignored otherwise.
    #[serde(default)]
    pub url: Option<String>,
    /// Name of the environment variable holding the API key. Read at resolution time.
    /// Omitted = keyless (valid only for local `openai` endpoints).
    #[serde(default)]
    pub key_env: Option<String>,
}

/// The parsed `.devtribunal.yml`.
#[derive(Debug, Clone, Deserialize)]
pub struct RoutingConfig {
    #[serde(default)]
    pub default: Option<RouteConfig>,
    #[serde(default)]
    pub routes: HashMap<String, RouteConfig>,
}

/// Walk up from `start_path` looking for a `.devtribunal.yml` file (mirrors
/// [`crate::types::resolve_agents_dir`]). A malformed or unreadable file is treated as
/// absent (logged, returns `None`) so the env path stays intact and the server still starts.
pub fn load_routing(start_path: &str, is_directory: bool) -> Option<RoutingConfig> {
    let start = std::path::Path::new(start_path);
    let mut dir = if is_directory {
        start.to_path_buf()
    } else {
        start.parent()?.to_path_buf()
    };

    loop {
        let candidate = dir.join(".devtribunal.yml");
        if candidate.is_file() {
            return match std::fs::read_to_string(&candidate) {
                Ok(text) => match serde_yaml::from_str::<RoutingConfig>(&text) {
                    Ok(cfg) => Some(cfg),
                    Err(e) => {
                        tracing::warn!(
                            "ignoring malformed routing config {}: {e}",
                            candidate.display()
                        );
                        None
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        "ignoring unreadable routing config {}: {e}",
                        candidate.display()
                    );
                    None
                }
            };
        }
        if !dir.pop() {
            break;
        }
    }

    None
}

/// Resolve the effective backend config for `agent`.
///
/// - `routing` `None`, or no matching route and no `default` ⇒ `env_default` unchanged.
/// - Otherwise the matched route is mapped to a [`BackendConfig`]; misconfiguration
///   (missing url/model, unset `key_env`) degrades to host mode with a `fallback_warning`.
pub fn resolve_backend_for_agent(
    agent: &str,
    routing: Option<&RoutingConfig>,
    env_default: &BackendConfig,
) -> BackendConfig {
    let Some(routing) = routing else {
        return env_default.clone();
    };
    let route = routing.routes.get(agent).or(routing.default.as_ref());
    match route {
        Some(route) => route_to_backend_config(agent, route, env_default),
        None => env_default.clone(),
    }
}

fn route_to_backend_config(
    agent: &str,
    route: &RouteConfig,
    env_default: &BackendConfig,
) -> BackendConfig {
    match route.provider {
        Provider::Host => BackendConfig {
            backend: Backend::Host,
            api_key: None,
            model: route
                .model
                .clone()
                .unwrap_or_else(|| env_default.model.clone()),
            local_url: None,
            local_model: None,
            fallback_warning: None,
        },
        Provider::Anthropic => {
            let key_env = route.key_env.as_deref().unwrap_or("DEVTRIBUNAL_API_KEY");
            let model = route
                .model
                .clone()
                .unwrap_or_else(|| env_default.model.clone());
            match env_nonempty(key_env) {
                Some(api_key) => BackendConfig {
                    backend: Backend::Api,
                    api_key: Some(api_key),
                    model,
                    local_url: None,
                    local_model: None,
                    fallback_warning: None,
                },
                None => host_fallback(
                    agent,
                    &format!("anthropic route key_env '{key_env}' is not set"),
                    env_default,
                ),
            }
        }
        Provider::Openai => {
            let Some(url) = route.url.clone() else {
                return host_fallback(agent, "openai route is missing 'url'", env_default);
            };
            let Some(model) = route.model.clone() else {
                return host_fallback(agent, "openai route is missing 'model'", env_default);
            };
            // key_env present ⇒ must resolve to a non-empty value; absent ⇒ keyless (local).
            let api_key = match route.key_env.as_deref() {
                Some(key_env) => match env_nonempty(key_env) {
                    Some(k) => Some(k),
                    None => {
                        return host_fallback(
                            agent,
                            &format!("openai route key_env '{key_env}' is not set"),
                            env_default,
                        )
                    }
                },
                None => None,
            };
            BackendConfig {
                backend: Backend::Openai,
                api_key,
                model: model.clone(),
                local_url: Some(url),
                local_model: Some(model),
                fallback_warning: None,
            }
        }
    }
}

fn host_fallback(agent: &str, reason: &str, env_default: &BackendConfig) -> BackendConfig {
    let warning = format!("WARNING: routing for '{agent}' degraded to host mode: {reason}.");
    tracing::error!("{warning}");
    BackendConfig {
        backend: Backend::Host,
        api_key: None,
        model: env_default.model.clone(),
        local_url: None,
        local_model: None,
        fallback_warning: Some(warning),
    }
}

/// Read an env var, trimmed, returning `None` when unset or empty.
fn env_nonempty(var: &str) -> Option<String> {
    std::env::var(var)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env vars are process-global; serialize tests that mutate them. Routing tests use
    // ROUTING_TEST_* var names (never DEVTRIBUNAL_*) so they cannot collide with backend.rs tests.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_default() -> BackendConfig {
        BackendConfig {
            backend: Backend::Host,
            api_key: None,
            model: "env-default-model".to_string(),
            local_url: None,
            local_model: None,
            fallback_warning: None,
        }
    }

    fn parse(yaml: &str) -> RoutingConfig {
        serde_yaml::from_str(yaml).expect("valid routing yaml")
    }

    // --- AC-1: precedence / fallback ---

    #[test]
    fn no_routing_returns_env_default_unchanged() {
        let cfg = resolve_backend_for_agent("review_rust", None, &env_default());
        assert_eq!(cfg.backend, Backend::Host);
        assert_eq!(cfg.model, "env-default-model");
        assert!(cfg.fallback_warning.is_none());
    }

    #[test]
    fn no_matching_route_and_no_default_returns_env_default() {
        let routing = parse("routes:\n  review_rust:\n    provider: host\n");
        let cfg = resolve_backend_for_agent("review_go", Some(&routing), &env_default());
        // Falls through to env_default (model carried from env_default).
        assert_eq!(cfg.backend, Backend::Host);
        assert_eq!(cfg.model, "env-default-model");
    }

    #[test]
    fn specific_route_beats_default() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::set_var("ROUTING_TEST_ANTHROPIC", "sk-ant-x");
        let routing = parse(
            "default:\n  provider: anthropic\n  model: claude-default\n  key_env: ROUTING_TEST_ANTHROPIC\n\
             routes:\n  review_rust:\n    provider: host\n    model: host-rust\n",
        );
        let specific = resolve_backend_for_agent("review_rust", Some(&routing), &env_default());
        assert_eq!(specific.backend, Backend::Host);
        assert_eq!(specific.model, "host-rust");

        let defaulted = resolve_backend_for_agent("review_go", Some(&routing), &env_default());
        assert_eq!(defaulted.backend, Backend::Api);
        assert_eq!(defaulted.model, "claude-default");
        std::env::remove_var("ROUTING_TEST_ANTHROPIC");
    }

    // --- AC-2: provider mapping + key_env ---

    #[test]
    fn anthropic_provider_maps_to_api_with_key_from_env() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::set_var("ROUTING_TEST_ANTHROPIC", "sk-ant-123");
        let routing = parse(
            "routes:\n  architect:\n    provider: anthropic\n    model: claude-opus-x\n    key_env: ROUTING_TEST_ANTHROPIC\n",
        );
        let cfg = resolve_backend_for_agent("architect", Some(&routing), &env_default());
        assert_eq!(cfg.backend, Backend::Api);
        assert_eq!(cfg.model, "claude-opus-x");
        assert_eq!(cfg.api_key.as_deref(), Some("sk-ant-123"));
        std::env::remove_var("ROUTING_TEST_ANTHROPIC");
    }

    #[test]
    fn openai_provider_keyed_maps_to_openai() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::set_var("ROUTING_TEST_XAI", "xai-key");
        let routing = parse(
            "routes:\n  review_rust:\n    provider: openai\n    model: grok-code\n    url: https://api.x.ai/v1\n    key_env: ROUTING_TEST_XAI\n",
        );
        let cfg = resolve_backend_for_agent("review_rust", Some(&routing), &env_default());
        assert_eq!(cfg.backend, Backend::Openai);
        assert_eq!(cfg.local_url.as_deref(), Some("https://api.x.ai/v1"));
        assert_eq!(cfg.local_model.as_deref(), Some("grok-code"));
        assert_eq!(cfg.model, "grok-code");
        assert_eq!(cfg.api_key.as_deref(), Some("xai-key"));
        std::env::remove_var("ROUTING_TEST_XAI");
    }

    #[test]
    fn openai_provider_keyless_is_local_no_key() {
        let routing = parse(
            "routes:\n  review_python:\n    provider: openai\n    model: qwen3:32b\n    url: http://localhost:11434/v1\n",
        );
        let cfg = resolve_backend_for_agent("review_python", Some(&routing), &env_default());
        assert_eq!(cfg.backend, Backend::Openai);
        assert_eq!(cfg.local_url.as_deref(), Some("http://localhost:11434/v1"));
        assert_eq!(cfg.local_model.as_deref(), Some("qwen3:32b"));
        assert!(cfg.api_key.is_none());
    }

    #[test]
    fn host_provider_maps_to_host() {
        let routing = parse("routes:\n  review_go:\n    provider: host\n");
        let cfg = resolve_backend_for_agent("review_go", Some(&routing), &env_default());
        assert_eq!(cfg.backend, Backend::Host);
    }

    // --- AC-3: misconfiguration degrades to host + warning; malformed file = absent ---

    #[test]
    fn anthropic_with_unset_key_env_degrades_to_host() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::remove_var("ROUTING_TEST_MISSING");
        let routing = parse(
            "routes:\n  architect:\n    provider: anthropic\n    key_env: ROUTING_TEST_MISSING\n",
        );
        let cfg = resolve_backend_for_agent("architect", Some(&routing), &env_default());
        assert_eq!(cfg.backend, Backend::Host);
        let w = cfg.fallback_warning.expect("warning present");
        assert!(w.contains("architect"));
        assert!(w.contains("ROUTING_TEST_MISSING"));
    }

    #[test]
    fn openai_missing_url_degrades_to_host() {
        let routing =
            parse("routes:\n  review_rust:\n    provider: openai\n    model: grok-code\n");
        let cfg = resolve_backend_for_agent("review_rust", Some(&routing), &env_default());
        assert_eq!(cfg.backend, Backend::Host);
        assert!(cfg.fallback_warning.unwrap().contains("url"));
    }

    #[test]
    fn openai_missing_model_degrades_to_host() {
        let routing = parse(
            "routes:\n  review_rust:\n    provider: openai\n    url: http://localhost:11434/v1\n",
        );
        let cfg = resolve_backend_for_agent("review_rust", Some(&routing), &env_default());
        assert_eq!(cfg.backend, Backend::Host);
        assert!(cfg.fallback_warning.unwrap().contains("model"));
    }

    #[test]
    fn load_routing_reads_valid_file_and_walks_up() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join(".devtribunal.yml"),
            "routes:\n  review_rust:\n    provider: host\n",
        )
        .unwrap();
        let nested = root.join("src").join("deep");
        std::fs::create_dir_all(&nested).unwrap();

        // From a file path nested below the config, walk up and find it.
        let file = nested.join("main.rs");
        let routing = load_routing(file.to_str().unwrap(), false).expect("config found");
        assert!(routing.routes.contains_key("review_rust"));

        // From a directory.
        let routing = load_routing(nested.to_str().unwrap(), true).expect("config found");
        assert!(routing.routes.contains_key("review_rust"));
    }

    #[test]
    fn load_routing_malformed_file_is_treated_as_absent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".devtribunal.yml"),
            "routes: [this is: not valid: yaml mapping\n",
        )
        .unwrap();
        let routing = load_routing(dir.path().to_str().unwrap(), true);
        assert!(routing.is_none());
    }

    #[test]
    fn load_routing_absent_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let routing = load_routing(dir.path().to_str().unwrap(), true);
        assert!(routing.is_none());
    }
}
