//! Multi-backend LLM support for devtribunal.
//!
//! Three modes:
//! - **host** (default): returns linter output only; the host LLM does the review.
//! - **api**: calls the Anthropic Messages API directly.
//! - **local**: calls an OpenAI-compatible local endpoint (ollama, llama.cpp, vllm).

use anyhow::Result;
use serde_json::json;

/// Which backend processes review prompts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    Host,
    Api,
    Local,
}

/// All configuration for the chosen backend.
#[derive(Debug, Clone)]
pub struct BackendConfig {
    pub backend: Backend,
    pub api_key: Option<String>,
    pub model: String,
    pub local_url: Option<String>,
    pub local_model: Option<String>,
    /// If the backend was downgraded to Host due to missing config, store the warning.
    pub fallback_warning: Option<String>,
}

/// Read environment variables and build configuration.
/// Falls back to Host mode with a warning if required vars are missing.
pub fn load_config() -> BackendConfig {
    let backend_str = std::env::var("DEVTRIBUNAL_BACKEND")
        .unwrap_or_else(|_| "host".to_string())
        .to_lowercase();

    let api_key = std::env::var("DEVTRIBUNAL_API_KEY").ok();
    let model = std::env::var("DEVTRIBUNAL_MODEL")
        .unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string());
    let local_url = std::env::var("DEVTRIBUNAL_LOCAL_URL").ok();
    let local_model = std::env::var("DEVTRIBUNAL_LOCAL_MODEL").ok();

    let (backend, fallback_warning) = match backend_str.as_str() {
        "api" => {
            if api_key.is_none() {
                tracing::error!(
                    "DEVTRIBUNAL_BACKEND=api but DEVTRIBUNAL_API_KEY is not set — falling back to host mode"
                );
                (
                    Backend::Host,
                    Some("WARNING: DEVTRIBUNAL_BACKEND=api but DEVTRIBUNAL_API_KEY is not set. Falling back to host mode.".to_string()),
                )
            } else {
                (Backend::Api, None)
            }
        }
        "local" => {
            if local_url.is_none() {
                tracing::error!(
                    "DEVTRIBUNAL_BACKEND=local but DEVTRIBUNAL_LOCAL_URL is not set — falling back to host mode"
                );
                (
                    Backend::Host,
                    Some("WARNING: DEVTRIBUNAL_BACKEND=local but DEVTRIBUNAL_LOCAL_URL is not set. Falling back to host mode.".to_string()),
                )
            } else if local_model.is_none() {
                tracing::error!(
                    "DEVTRIBUNAL_BACKEND=local but DEVTRIBUNAL_LOCAL_MODEL is not set — falling back to host mode"
                );
                (
                    Backend::Host,
                    Some("WARNING: DEVTRIBUNAL_BACKEND=local but DEVTRIBUNAL_LOCAL_MODEL is not set. Falling back to host mode.".to_string()),
                )
            } else {
                (Backend::Local, None)
            }
        }
        _ => (Backend::Host, None),
    };

    BackendConfig {
        backend,
        api_key,
        model,
        local_url,
        local_model,
        fallback_warning,
    }
}

/// Returns a one-line mode indicator string to prepend to tool results.
pub fn mode_indicator(config: &BackendConfig) -> String {
    match config.backend {
        Backend::Host => {
            "[devtribunal \u{00b7} host mode \u{00b7} review processed by your Claude session]".to_string()
        }
        Backend::Api => {
            format!(
                "[devtribunal \u{00b7} api mode \u{00b7} {} \u{00b7} billed to your API key]",
                config.model
            )
        }
        Backend::Local => {
            let model = config.local_model.as_deref().unwrap_or("unknown");
            let url = config.local_url.as_deref().unwrap_or("unknown");
            // Extract host:port from URL for concise display
            let host = url
                .strip_prefix("http://")
                .or_else(|| url.strip_prefix("https://"))
                .unwrap_or(url)
                .trim_end_matches("/v1")
                .trim_end_matches('/');
            format!(
                "[devtribunal \u{00b7} local mode \u{00b7} {} @ {}]",
                model, host
            )
        }
    }
}

/// Process a review prompt through the configured backend.
///
/// - **Host**: returns `Ok(None)` — the caller should return linter output for the host LLM.
/// - **Api**: POSTs to Anthropic Messages API and returns the response text.
/// - **Local**: POSTs to the OpenAI-compatible endpoint and returns the response text.
pub async fn process_review(config: &BackendConfig, prompt: &str) -> Result<Option<String>> {
    match config.backend {
        Backend::Host => Ok(None),
        Backend::Api => call_anthropic_api(config, prompt).await.map(Some),
        Backend::Local => call_local_api(config, prompt).await.map(Some),
    }
}

/// Call the Anthropic Messages API.
async fn call_anthropic_api(config: &BackendConfig, prompt: &str) -> Result<String> {
    let api_key = config
        .api_key
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("API key not configured"))?;

    let client = reqwest::Client::new();
    let body = json!({
        "model": config.model,
        "max_tokens": 4096,
        "messages": [
            {"role": "user", "content": prompt}
        ]
    });

    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        anyhow::bail!(
            "Anthropic API returned HTTP {}: {}",
            status.as_u16(),
            error_body
        );
    }

    let resp_json: serde_json::Value = response.json().await?;

    // Extract content[0].text
    let text = resp_json
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|block| block.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    if text.is_empty() {
        anyhow::bail!("Anthropic API returned empty content. Response: {resp_json}");
    }

    Ok(text)
}

/// Call an OpenAI-compatible local endpoint.
async fn call_local_api(config: &BackendConfig, prompt: &str) -> Result<String> {
    let base_url = config
        .local_url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Local URL not configured"))?;
    let model = config
        .local_model
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Local model not configured"))?;

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let client = reqwest::Client::new();
    let body = json!({
        "model": model,
        "messages": [
            {"role": "user", "content": prompt}
        ],
        "max_tokens": 4096
    });

    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        anyhow::bail!(
            "Local LLM endpoint returned HTTP {}: {}",
            status.as_u16(),
            error_body
        );
    }

    let resp_json: serde_json::Value = response.json().await?;

    // Extract choices[0].message.content
    let text = resp_json
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|msg| msg.get("content"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    if text.is_empty() {
        anyhow::bail!("Local LLM endpoint returned empty content. Response: {resp_json}");
    }

    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env vars are process-global; tests that mutate them must be serialized.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_env() {
        std::env::remove_var("DEVTRIBUNAL_BACKEND");
        std::env::remove_var("DEVTRIBUNAL_API_KEY");
        std::env::remove_var("DEVTRIBUNAL_MODEL");
        std::env::remove_var("DEVTRIBUNAL_LOCAL_URL");
        std::env::remove_var("DEVTRIBUNAL_LOCAL_MODEL");
    }

    #[test]
    fn test_load_config_defaults_to_host() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_env();

        let config = load_config();
        assert_eq!(config.backend, Backend::Host);
        assert!(config.fallback_warning.is_none());
    }

    #[test]
    fn test_load_config_api_without_key_falls_back() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_env();
        std::env::set_var("DEVTRIBUNAL_BACKEND", "api");

        let config = load_config();
        assert_eq!(config.backend, Backend::Host);
        assert!(config.fallback_warning.is_some());
        assert!(config.fallback_warning.unwrap().contains("API_KEY"));

        clear_env();
    }

    #[test]
    fn test_load_config_api_with_key() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_env();
        std::env::set_var("DEVTRIBUNAL_BACKEND", "api");
        std::env::set_var("DEVTRIBUNAL_API_KEY", "sk-ant-test123");

        let config = load_config();
        assert_eq!(config.backend, Backend::Api);
        assert!(config.fallback_warning.is_none());
        assert_eq!(config.api_key.as_deref(), Some("sk-ant-test123"));

        clear_env();
    }

    #[test]
    fn test_load_config_local_without_url_falls_back() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_env();
        std::env::set_var("DEVTRIBUNAL_BACKEND", "local");
        std::env::set_var("DEVTRIBUNAL_LOCAL_MODEL", "qwen3:32b");

        let config = load_config();
        assert_eq!(config.backend, Backend::Host);
        assert!(config.fallback_warning.is_some());
        assert!(config.fallback_warning.unwrap().contains("LOCAL_URL"));

        clear_env();
    }

    #[test]
    fn test_load_config_local_without_model_falls_back() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_env();
        std::env::set_var("DEVTRIBUNAL_BACKEND", "local");
        std::env::set_var("DEVTRIBUNAL_LOCAL_URL", "http://localhost:11434/v1");

        let config = load_config();
        assert_eq!(config.backend, Backend::Host);
        assert!(config.fallback_warning.is_some());
        assert!(config.fallback_warning.unwrap().contains("LOCAL_MODEL"));

        clear_env();
    }

    #[test]
    fn test_load_config_local_complete() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_env();
        std::env::set_var("DEVTRIBUNAL_BACKEND", "local");
        std::env::set_var("DEVTRIBUNAL_LOCAL_URL", "http://localhost:11434/v1");
        std::env::set_var("DEVTRIBUNAL_LOCAL_MODEL", "qwen3:32b");

        let config = load_config();
        assert_eq!(config.backend, Backend::Local);
        assert!(config.fallback_warning.is_none());
        assert_eq!(config.local_url.as_deref(), Some("http://localhost:11434/v1"));
        assert_eq!(config.local_model.as_deref(), Some("qwen3:32b"));

        clear_env();
    }

    #[test]
    fn test_mode_indicator_host() {
        let config = BackendConfig {
            backend: Backend::Host,
            api_key: None,
            model: "claude-sonnet-4-20250514".to_string(),
            local_url: None,
            local_model: None,
            fallback_warning: None,
        };
        let indicator = mode_indicator(&config);
        assert!(indicator.contains("host mode"));
        assert!(indicator.contains("review processed by your Claude session"));
    }

    #[test]
    fn test_mode_indicator_api() {
        let config = BackendConfig {
            backend: Backend::Api,
            api_key: Some("sk-ant-test".to_string()),
            model: "claude-sonnet-4-20250514".to_string(),
            local_url: None,
            local_model: None,
            fallback_warning: None,
        };
        let indicator = mode_indicator(&config);
        assert!(indicator.contains("api mode"));
        assert!(indicator.contains("claude-sonnet-4-20250514"));
        assert!(indicator.contains("billed to your API key"));
    }

    #[test]
    fn test_mode_indicator_local() {
        let config = BackendConfig {
            backend: Backend::Local,
            api_key: None,
            model: "claude-sonnet-4-20250514".to_string(),
            local_url: Some("http://localhost:11434/v1".to_string()),
            local_model: Some("qwen3:32b".to_string()),
            fallback_warning: None,
        };
        let indicator = mode_indicator(&config);
        assert!(indicator.contains("local mode"));
        assert!(indicator.contains("qwen3:32b"));
        assert!(indicator.contains("localhost:11434"));
    }

    #[test]
    fn test_default_model() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_env();

        let config = load_config();
        assert_eq!(config.model, "claude-sonnet-4-20250514");

        clear_env();
    }

    #[tokio::test]
    async fn test_process_review_host_returns_none() {
        let config = BackendConfig {
            backend: Backend::Host,
            api_key: None,
            model: "claude-sonnet-4-20250514".to_string(),
            local_url: None,
            local_model: None,
            fallback_warning: None,
        };
        let result = process_review(&config, "test prompt").await.unwrap();
        assert!(result.is_none());
    }
}
