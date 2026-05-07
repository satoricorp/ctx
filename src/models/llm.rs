use anyhow::{anyhow, bail, Context, Result};
use reqwest::blocking::Client;

use crate::install::load_config;

pub async fn complete_json(prompt: &str) -> Result<String> {
    let prompt = prompt.to_owned();
    tokio::task::spawn_blocking(move || complete_json_sync(&prompt)).await?
}

pub fn configured_extraction_model() -> String {
    let configured = load_config()
        .map(|config| config.extraction_model)
        .unwrap_or_else(|_| String::from("openai:gpt-5.4-nano"));
    if configured.starts_with("openai:") {
        configured
    } else {
        String::from("openai:gpt-5.4-nano")
    }
}

pub fn should_use_cloud_extraction() -> bool {
    configured_extraction_model().starts_with("openai:") && openai_api_key_present()
}

pub fn should_warn_missing_extraction_api_key() -> bool {
    configured_extraction_model().starts_with("openai:") && !openai_api_key_present()
}

pub fn warn_missing_extraction_api_key_once() {
    use std::sync::OnceLock;
    static WARNED: OnceLock<()> = OnceLock::new();
    if WARNED.set(()).is_ok() {
        eprintln!(
            "ctx: extraction_model is {} but OPENAI_API_KEY is unset. using heuristic extraction.",
            configured_extraction_model()
        );
    }
}

pub fn configured_backend_label() -> String {
    configured_extraction_model()
}

fn openai_api_key_present() -> bool {
    std::env::var("OPENAI_API_KEY")
        .ok()
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false)
}

fn complete_json_sync(prompt: &str) -> Result<String> {
    let model = configured_extraction_model();
    let model_id = model
        .strip_prefix("openai:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow!("unsupported extraction_model {model}; only openai:* is supported")
        })?;
    openai_chat_json(prompt, model_id)
}

fn openai_chat_uses_reasoning_fields(model: &str) -> bool {
    model.to_ascii_lowercase().contains("gpt-5")
}

fn openai_base_url() -> String {
    std::env::var("CTX_OPENAI_BASE_URL")
        .ok()
        .map(|value| value.trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| String::from("https://api.openai.com/v1"))
}

fn openai_chat_json(prompt: &str, model: &str) -> Result<String> {
    let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| {
        anyhow!("OPENAI_API_KEY is not set (required for extraction model openai:{model})")
    })?;

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .context("failed to build HTTP client for OpenAI")?;

    let body = if openai_chat_uses_reasoning_fields(model) {
        serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "max_completion_tokens": 4096,
            "reasoning_effort": "low",
            "response_format": {"type": "json_object"},
        })
    } else {
        serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": 4096,
            "temperature": 0.2,
            "response_format": {"type": "json_object"},
        })
    };

    let response = client
        .post(format!("{}/chat/completions", openai_base_url()))
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .context("openai chat completions request failed")?;

    let status = response.status();
    let body_text = response
        .text()
        .context("failed to read OpenAI response body")?;
    if !status.is_success() {
        bail!("OpenAI API error ({}): {}", status, body_text.trim());
    }

    if status.as_u16() == 200 {
        super::openai_ok::log_openai_success("chat/completions");
    }

    let v: serde_json::Value =
        serde_json::from_str(&body_text).context("failed to parse OpenAI JSON response")?;
    let text = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| {
            anyhow!("unexpected OpenAI response shape (missing choices[0].message.content)")
        })?;
    Ok(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warns_when_openai_model_has_no_key() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let saved = std::env::var("OPENAI_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");
        assert!(should_warn_missing_extraction_api_key());
        if let Some(value) = saved {
            std::env::set_var("OPENAI_API_KEY", value);
        }
    }

    #[test]
    fn base_url_override_trims_trailing_slash() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        std::env::set_var("CTX_OPENAI_BASE_URL", "http://127.0.0.1:9999/v1/");
        assert_eq!(openai_base_url(), "http://127.0.0.1:9999/v1");
        std::env::remove_var("CTX_OPENAI_BASE_URL");
    }
}
