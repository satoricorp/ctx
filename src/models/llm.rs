use anyhow::{anyhow, bail, Context, Result};
use encoding_rs::UTF_8;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use reqwest::blocking::Client;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once, OnceLock};

static LOCAL_LLAMA_INFERENCE_LOCK: Mutex<()> = Mutex::new(());
use std::time::Instant;

use crate::install::{load_config, resolve_models_dir, Config};

const LOCAL_EXTRACTION_SMALL: &str = "gemma4-e4b";
const LOCAL_EXTRACTION_LARGE: &str = "gemma4-26b-a4b";
const DEFAULT_MAX_TOKENS: usize = 768;
const DEFAULT_TIMEOUT_MS: u64 = 45_000;
const DEFAULT_N_CTX: u32 = 8_192;

#[derive(Debug, Clone, Copy)]
struct LocalModelSpec {
    model_id: &'static str,
    file_name: &'static str,
    url: &'static str,
    size_hint: &'static str,
}

const LOCAL_MODEL_SPECS: &[LocalModelSpec] = &[
    LocalModelSpec {
        model_id: LOCAL_EXTRACTION_SMALL,
        file_name: "gemma-4-e4b-it-Q4_K_M.gguf",
        url: "https://huggingface.co/ggml-org/gemma-4-E4B-it-GGUF/resolve/main/gemma-4-e4b-it-Q4_K_M.gguf?download=1",
        size_hint: "~3GB",
    },
    LocalModelSpec {
        model_id: LOCAL_EXTRACTION_LARGE,
        file_name: "gemma-4-26B-A4B-it-Q4_K_M.gguf",
        url: "https://huggingface.co/ggml-org/gemma-4-26B-A4B-it-GGUF/resolve/main/gemma-4-26B-A4B-it-Q4_K_M.gguf?download=1",
        size_hint: "~8GB",
    },
];

#[derive(Debug, Clone, Copy)]
struct LlamaRuntimeConfig {
    max_tokens: usize,
    timeout_ms: u64,
    n_ctx: u32,
}

pub async fn complete_json(prompt: &str) -> Result<String> {
    let prompt = prompt.to_owned();
    tokio::task::spawn_blocking(move || complete_json_sync(&prompt)).await?
}

pub fn configured_extraction_model() -> String {
    load_config()
        .map(|config| config.extraction_model)
        .unwrap_or_else(|_| String::from("openai:gpt-4o"))
}

pub fn should_use_local_llm() -> bool {
    local_model_spec(configured_extraction_model().as_str()).is_some()
}

/// True when config selects OpenAI or Anthropic and the matching API key is present.
pub fn should_use_cloud_extraction() -> bool {
    let configured = configured_extraction_model();
    if configured.starts_with("openai:") {
        return openai_api_key_present();
    }
    if configured.starts_with("anthropic:") {
        return anthropic_api_key_present();
    }
    false
}

fn openai_api_key_present() -> bool {
    std::env::var("OPENAI_API_KEY")
        .ok()
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false)
}

fn anthropic_api_key_present() -> bool {
    std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false)
}

/// Config names OpenAI or Anthropic extraction but the matching env key is unset.
pub fn should_warn_missing_extraction_api_key() -> bool {
    let configured = configured_extraction_model();
    if configured.starts_with("openai:") {
        return !openai_api_key_present();
    }
    if configured.starts_with("anthropic:") {
        return !anthropic_api_key_present();
    }
    false
}

pub fn warn_missing_extraction_api_key_once() {
    use std::sync::OnceLock;
    static WARNED: OnceLock<()> = OnceLock::new();
    if WARNED.set(()).is_ok() {
        let configured = configured_extraction_model();
        let hint = if configured.starts_with("openai:") {
            "set OPENAI_API_KEY"
        } else if configured.starts_with("anthropic:") {
            "set ANTHROPIC_API_KEY"
        } else {
            "set the matching API key"
        };
        eprintln!(
            "ctx: extraction_model is {} but {}. using heuristic extraction.",
            configured, hint
        );
    }
}

pub fn configured_backend_label() -> String {
    configured_extraction_model()
}

pub fn ensure_local_extraction_model(model_id: &str, show_progress: bool) -> Result<PathBuf> {
    let config = load_config().unwrap_or_default();
    ensure_local_extraction_model_for_config(&config, model_id, show_progress)
}

pub fn local_model_path(model_id: &str, models_dir: &Path) -> Result<PathBuf> {
    let spec = local_model_spec(model_id)
        .ok_or_else(|| anyhow!("unsupported local extraction model {}", model_id))?;
    Ok(models_dir.join(spec.file_name))
}

fn complete_json_sync(prompt: &str) -> Result<String> {
    let config = load_config().unwrap_or_default();
    let model = config.extraction_model.as_str();
    if let Some(rest) = model.strip_prefix("openai:") {
        let model_id = rest.trim();
        if model_id.is_empty() {
            bail!("invalid extraction_model: empty openai model id");
        }
        return openai_chat_json(prompt, model_id);
    }
    if let Some(rest) = model.strip_prefix("anthropic:") {
        let model_id = rest.trim();
        if model_id.is_empty() {
            bail!("invalid extraction_model: empty anthropic model id");
        }
        return anthropic_messages_json(prompt, model_id);
    }

    let model_path = ensure_local_extraction_model_for_config(&config, model, false)?;
    // llama.cpp contexts must not decode concurrently on the same model; keep completions serial.
    let _guard = LOCAL_LLAMA_INFERENCE_LOCK
        .lock()
        .map_err(|_| anyhow!("local llama inference lock poisoned"))?;
    run_llama_completion(&model_path, prompt, runtime_config_from_env())
}

fn openai_chat_json(prompt: &str, model: &str) -> Result<String> {
    let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| {
        anyhow!("OPENAI_API_KEY is not set (required for extraction model openai:{model})")
    })?;

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .context("failed to build HTTP client for OpenAI")?;

    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": 4096,
        "temperature": 0.2,
        "response_format": {"type": "json_object"},
    });

    let response = client
        .post("https://api.openai.com/v1/chat/completions")
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

    let v: serde_json::Value =
        serde_json::from_str(&body_text).context("failed to parse OpenAI JSON response")?;
    let text = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow!("unexpected OpenAI response shape (missing choices[0].message.content)"))?;
    Ok(text.to_string())
}

fn anthropic_messages_json(prompt: &str, model: &str) -> Result<String> {
    let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
        anyhow!(
            "ANTHROPIC_API_KEY is not set (required for extraction model anthropic:{model})"
        )
    })?;

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .context("failed to build HTTP client for Anthropic")?;

    let body = serde_json::json!({
        "model": model,
        "max_tokens": 4096,
        "messages": [{"role": "user", "content": prompt}],
    });

    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .context("anthropic messages request failed")?;

    let status = response.status();
    let body_text = response
        .text()
        .context("failed to read Anthropic response body")?;
    if !status.is_success() {
        bail!("Anthropic API error ({}): {}", status, body_text.trim());
    }

    let v: serde_json::Value =
        serde_json::from_str(&body_text).context("failed to parse Anthropic JSON response")?;
    let content = v
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|blocks| blocks.first())
        .ok_or_else(|| anyhow!("unexpected Anthropic response shape (missing content[0])"))?;
    let text = content
        .get("text")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!("unexpected Anthropic content block (missing text)"))?;
    Ok(text.to_string())
}

fn ensure_local_extraction_model_for_config(
    config: &Config,
    model_id: &str,
    show_progress: bool,
) -> Result<PathBuf> {
    let models_dir = resolve_models_dir(config);
    let spec = local_model_spec(model_id)
        .ok_or_else(|| anyhow!("unsupported local extraction model {}", model_id))?;
    fs::create_dir_all(&models_dir)
        .with_context(|| format!("failed to create {}", models_dir.display()))?;

    let path = models_dir.join(spec.file_name);
    if path.exists() {
        return Ok(path);
    }

    if std::env::var("CTX_SKIP_LLAMA_DOWNLOAD").ok().as_deref() == Some("1") {
        bail!(
            "{} is not installed and CTX_SKIP_LLAMA_DOWNLOAD=1 is set",
            spec.model_id
        );
    }

    if show_progress {
        println!(
            "ctx: downloading local extraction model {} ({})",
            spec.model_id, spec.size_hint
        );
    }

    download_model(spec, &path, show_progress)?;
    Ok(path)
}

fn download_model(spec: LocalModelSpec, destination: &Path, show_progress: bool) -> Result<()> {
    // Large GGUFs take a long time; reqwest's default timeout is too short for multi‑GB files.
    const DOWNLOAD_TIMEOUT: std::time::Duration =
        std::time::Duration::from_secs(7 * 24 * 3600);
    let client = Client::builder()
        .user_agent("ctx/0.1")
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .context("failed to build download client")?;
    let response = client
        .get(spec.url)
        .send()
        .with_context(|| {
            format!(
                "failed to connect for {} (URL {})",
                spec.model_id, spec.url
            )
        })?;

    let status = response.status();
    let mut response = if !status.is_success() {
        let err_text = response
            .text()
            .unwrap_or_else(|e| format!("(could not read error body: {e})"));
        bail!(
            "HTTP {} downloading {} — {}\n{}",
            status.as_u16(),
            spec.model_id,
            spec.url,
            err_text.chars().take(1200).collect::<String>()
        );
    } else {
        response
    };

    let tmp_path = destination.with_extension("gguf.part");
    let mut file = fs::File::create(&tmp_path)
        .with_context(|| format!("failed to create {}", tmp_path.display()))?;
    let mut downloaded = 0_u64;
    let mut buffer = [0_u8; 1024 * 1024];
    let total = response.content_length();
    let mut last_reported_mb = 0_u64;

    loop {
        let read = response
            .read(&mut buffer)
            .with_context(|| format!("failed while downloading {}", spec.model_id))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .with_context(|| format!("failed writing {}", tmp_path.display()))?;
        downloaded += read as u64;

        if show_progress {
            let downloaded_mb = downloaded / (1024 * 1024);
            if downloaded_mb >= last_reported_mb + 256 {
                last_reported_mb = downloaded_mb;
                if let Some(total) = total {
                    let total_mb = total / (1024 * 1024);
                    println!(
                        "ctx: downloaded {} MB / {} MB for {}",
                        downloaded_mb, total_mb, spec.model_id
                    );
                } else {
                    println!("ctx: downloaded {} MB for {}", downloaded_mb, spec.model_id);
                }
            }
        }
    }

    file.flush()
        .with_context(|| format!("failed to flush {}", tmp_path.display()))?;
    fs::rename(&tmp_path, destination).with_context(|| {
        format!(
            "failed to move {} into place at {}",
            tmp_path.display(),
            destination.display()
        )
    })?;

    if show_progress {
        println!(
            "ctx: local extraction model ready at {}",
            destination.display()
        );
    }

    Ok(())
}

fn run_llama_completion(
    model_path: &Path,
    prompt: &str,
    runtime: LlamaRuntimeConfig,
) -> Result<String> {
    let backend = llama_backend()?;
    let model = cached_llama_model(backend, model_path)?;
    let n_ctx = runtime.n_ctx.max(512).min(32_768);
    let mut ctx_params = LlamaContextParams::default()
        .with_n_ctx(Some(
            NonZeroU32::new(n_ctx).ok_or_else(|| anyhow!("invalid n_ctx"))?,
        ))
        .with_n_batch(n_ctx);
    let threads = std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(4);
    ctx_params = ctx_params.with_n_threads(threads as i32);
    ctx_params = ctx_params.with_n_threads_batch(threads as i32);

    let mut context = model
        .new_context(backend, ctx_params)
        .map_err(|error| anyhow!("failed to initialize llama context: {}", error))?;

    let mut prompt_tokens = model
        .str_to_token(prompt, AddBos::Always)
        .map_err(|error| anyhow!("failed to tokenize prompt: {}", error))?;
    if prompt_tokens.is_empty() {
        bail!("tokenized prompt is empty")
    }

    let max_prompt_tokens = (context.n_ctx() as usize).saturating_sub(runtime.max_tokens.max(64));
    if prompt_tokens.len() > max_prompt_tokens {
        prompt_tokens.truncate(max_prompt_tokens.max(1));
    }

    let n_batch = context.n_batch() as usize;
    let mut batch = LlamaBatch::new(
        usize::max(
            n_batch.min(prompt_tokens.len()) + runtime.max_tokens + 8,
            512,
        ),
        1,
    );
    let mut position = 0_i32;

    // Decode the prompt in chunks so we respect llama.cpp's batch limits and avoid
    // hard assertions in the native decode loop on larger prompts.
    while position < prompt_tokens.len() as i32 {
        let chunk_end = ((position as usize) + n_batch).min(prompt_tokens.len());
        let last_index = (chunk_end - 1) as i32;
        batch.clear();
        for (index, token) in
            (position..).zip(prompt_tokens[position as usize..chunk_end].iter().copied())
        {
            batch
                .add(token, index, &[0], index == last_index)
                .map_err(|error| anyhow!("failed adding prompt token to batch: {}", error))?;
        }
        context
            .decode(&mut batch)
            .map_err(|error| anyhow!("failed llama prompt decode: {}", error))?;
        position = chunk_end as i32;
    }

    let mut sampler = LlamaSampler::chain_simple([LlamaSampler::dist(42), LlamaSampler::greedy()]);
    let mut decoder = UTF_8.new_decoder();
    let mut output = String::new();
    let start = Instant::now();
    let mut cursor = prompt_tokens.len() as i32;

    for _ in 0..runtime.max_tokens.max(64) {
        if start.elapsed().as_millis() as u64 > runtime.timeout_ms {
            break;
        }

        let token = sampler.sample(&context, batch.n_tokens() - 1);
        sampler.accept(token);
        if model.is_eog_token(token) {
            break;
        }

        let piece = model
            .token_to_piece(token, &mut decoder, true, None)
            .map_err(|error| anyhow!("failed converting token to text: {}", error))?;
        output.push_str(&piece);
        if output.contains("<|user|>")
            || output.contains("<|assistant|>")
            || output.contains("|Human:")
            || output.contains("|ASSISTANT:")
        {
            break;
        }

        batch.clear();
        batch
            .add(token, cursor, &[0], true)
            .map_err(|error| anyhow!("failed preparing decode batch: {}", error))?;
        context
            .decode(&mut batch)
            .map_err(|error| anyhow!("failed llama decode loop: {}", error))?;
        cursor += 1;
    }

    if output.trim().is_empty() {
        bail!("llama output was empty")
    }

    Ok(output)
}

fn runtime_config_from_env() -> LlamaRuntimeConfig {
    let max_tokens = std::env::var("CTX_LLAMA_MAX_TOKENS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_MAX_TOKENS);
    let timeout_ms = std::env::var("CTX_LLAMA_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_MS);
    let n_ctx = std::env::var("CTX_LLAMA_N_CTX")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(DEFAULT_N_CTX);

    LlamaRuntimeConfig {
        max_tokens,
        timeout_ms,
        n_ctx,
    }
}

fn local_model_spec(model_id: &str) -> Option<LocalModelSpec> {
    LOCAL_MODEL_SPECS
        .iter()
        .copied()
        .find(|spec| spec.model_id == model_id)
}

fn cached_llama_model(backend: &LlamaBackend, model_path: &Path) -> Result<Arc<LlamaModel>> {
    static MODELS: OnceLock<Mutex<HashMap<String, Arc<LlamaModel>>>> = OnceLock::new();
    let cache = MODELS.get_or_init(|| Mutex::new(HashMap::new()));
    let model_key = model_path.display().to_string();

    if let Some(model) = cache
        .lock()
        .map_err(|_| anyhow!("llama model cache lock poisoned"))?
        .get(&model_key)
        .cloned()
    {
        return Ok(model);
    }

    let model = Arc::new(
        LlamaModel::load_from_file(backend, &model_key, &LlamaModelParams::default()).map_err(
            |error| {
                anyhow!(
                    "failed to load llama model {}: {}",
                    model_path.display(),
                    error
                )
            },
        )?,
    );
    let mut guard = cache
        .lock()
        .map_err(|_| anyhow!("llama model cache lock poisoned"))?;
    Ok(guard
        .entry(model_key)
        .or_insert_with(|| model.clone())
        .clone())
}

fn llama_backend() -> Result<&'static LlamaBackend> {
    static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();
    static INIT: Once = Once::new();
    static INIT_ERROR: Mutex<Option<anyhow::Error>> = Mutex::new(None);

    INIT.call_once(|| match LlamaBackend::init() {
        Ok(backend) => {
            BACKEND.set(backend).ok();
        }
        Err(error) => {
            *INIT_ERROR.lock().unwrap() =
                Some(anyhow!("failed to initialize llama backend: {}", error));
        }
    });

    BACKEND.get().ok_or_else(|| {
        INIT_ERROR
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| anyhow!("unknown llama backend init error"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_local_models() {
        assert!(local_model_spec(LOCAL_EXTRACTION_SMALL).is_some());
        assert!(local_model_spec(LOCAL_EXTRACTION_LARGE).is_some());
        assert!(local_model_spec("openai:gpt-4o").is_none());
    }

    #[test]
    fn runtime_env_overrides_parse() {
        std::env::set_var("CTX_LLAMA_MAX_TOKENS", "321");
        std::env::set_var("CTX_LLAMA_TIMEOUT_MS", "1234");
        std::env::set_var("CTX_LLAMA_N_CTX", "4096");
        let runtime = runtime_config_from_env();
        assert_eq!(runtime.max_tokens, 321);
        assert_eq!(runtime.timeout_ms, 1234);
        assert_eq!(runtime.n_ctx, 4096);
        std::env::remove_var("CTX_LLAMA_MAX_TOKENS");
        std::env::remove_var("CTX_LLAMA_TIMEOUT_MS");
        std::env::remove_var("CTX_LLAMA_N_CTX");
    }

    #[test]
    fn local_model_paths_live_under_models_dir() {
        let path = local_model_path(LOCAL_EXTRACTION_SMALL, Path::new("/tmp/models"))
            .expect("local model path");
        assert!(path.ends_with("gemma-4-e4b-it-Q4_K_M.gguf"));
    }
}
