use anyhow::{anyhow, Context, Result};
use fastembed::{
    EmbeddingModel, RerankInitOptions, RerankerModel, SparseInitOptions, SparseModel,
    SparseTextEmbedding, TextEmbedding, TextInitOptions, TextRerank,
};
use std::collections::{hash_map::DefaultHasher, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crate::install::{ensure_base_dirs, load_config, resolve_models_dir};

const HASH_DIMENSIONS: usize = 384;

enum EmbedderState {
    Uninitialized,
    Ready(TextEmbedding),
    Failed(String),
}

static EMBEDDER: OnceLock<Mutex<EmbedderState>> = OnceLock::new();
static FASTEMBED_FALLBACK_WARNED: OnceLock<()> = OnceLock::new();

pub async fn embed_dense(text: &str) -> Result<Vec<f32>> {
    let text = text.to_owned();
    tokio::task::spawn_blocking(move || embed_dense_sync(&text)).await?
}

pub fn install_required_fastembed_assets(
    cache_dir: &Path,
    show_download_progress: bool,
) -> Result<()> {
    fs::create_dir_all(cache_dir)?;
    let _embedder = create_dense_embedder(cache_dir, show_download_progress)?;
    let _reranker = create_reranker(cache_dir, show_download_progress)?;
    Ok(())
}

pub fn install_splade_asset(cache_dir: &Path, show_download_progress: bool) -> Result<()> {
    fs::create_dir_all(cache_dir)?;
    let _sparse = create_splade_embedder(cache_dir, show_download_progress)?;
    Ok(())
}

pub fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.is_empty() || right.is_empty() || left.len() != right.len() {
        return 0.0;
    }

    let (mut dot, mut left_norm, mut right_norm) = (0.0_f32, 0.0_f32, 0.0_f32);
    for (left_value, right_value) in left.iter().zip(right.iter()) {
        dot += left_value * right_value;
        left_norm += left_value * left_value;
        right_norm += right_value * right_value;
    }

    if left_norm == 0.0 || right_norm == 0.0 {
        return 0.0;
    }

    dot / (left_norm.sqrt() * right_norm.sqrt())
}

pub fn lexical_overlap(query: &str, text: &str) -> f32 {
    let query_terms: HashSet<String> = tokenize_terms(query).into_iter().collect();
    let text_terms: HashSet<String> = tokenize_terms(text).into_iter().collect();
    if query_terms.is_empty() || text_terms.is_empty() {
        return 0.0;
    }

    let overlap = query_terms.intersection(&text_terms).count() as f32;
    overlap / ((query_terms.len() as f32 * text_terms.len() as f32).sqrt())
}

pub fn tokenize_terms(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .map(|term| term.trim().to_lowercase())
        .filter(|term| term.len() >= 2)
        .collect()
}

fn embed_dense_sync(text: &str) -> Result<Vec<f32>> {
    let config = load_config().unwrap_or_default();
    if let Some(model) = config.embedding_model.strip_prefix("openai:") {
        let model = model.trim();
        if !model.is_empty() {
            if text.trim().is_empty() {
                return Ok(vec![0.0; openai_embedding_dimensions(model)]);
            }
            return openai_embed_sync(text.trim(), model);
        }
    }

    if text.trim().is_empty() {
        return Ok(vec![0.0; HASH_DIMENSIONS]);
    }

    if std::env::var("CTX_DISABLE_FASTEMBED").ok().as_deref() == Some("1") {
        return Ok(hash_embedding(text, HASH_DIMENSIONS));
    }

    match try_fastembed(text) {
        Ok(embedding) => Ok(embedding),
        Err(error) => {
            warn_fastembed_fallback(&error);
            Ok(hash_embedding(text, HASH_DIMENSIONS))
        }
    }
}

/// Default output dimensions for OpenAI embedding models (no `dimensions` API parameter).
fn openai_embedding_dimensions(model: &str) -> usize {
    let lower = model.to_ascii_lowercase();
    if lower.contains("3-large") {
        return 3072;
    }
    if lower.contains("3-small") {
        return 1536;
    }
    // ada-002 and most others
    1536
}

fn openai_embed_sync(text: &str, model: &str) -> Result<Vec<f32>> {
    let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| {
        anyhow!("OPENAI_API_KEY is not set (required for embedding_model openai:{model})")
    })?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .context("failed to build HTTP client for OpenAI embeddings")?;

    let body = serde_json::json!({
        "model": model,
        "input": text,
    });

    let response = client
        .post("https://api.openai.com/v1/embeddings")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .context("openai embeddings request failed")?;

    let status = response.status();
    let body_text = response
        .text()
        .context("failed to read OpenAI embeddings response body")?;
    if !status.is_success() {
        return Err(anyhow!(
            "OpenAI embeddings API error ({}): {}",
            status,
            body_text.trim()
        ));
    }

    let v: serde_json::Value =
        serde_json::from_str(&body_text).context("failed to parse OpenAI embeddings JSON")?;
    let arr = v["data"][0]["embedding"]
        .as_array()
        .ok_or_else(|| anyhow!("unexpected OpenAI embeddings response (missing data[0].embedding)"))?;
    Ok(arr
        .iter()
        .filter_map(|x| x.as_f64().map(|f| f as f32))
        .collect())
}

fn try_fastembed(text: &str) -> Result<Vec<f32>> {
    let embedder = EMBEDDER.get_or_init(|| Mutex::new(EmbedderState::Uninitialized));
    let mut state = embedder.lock().expect("embedder lock poisoned");

    if matches!(&*state, EmbedderState::Uninitialized) {
        ensure_base_dirs().ok();
        let config = load_config().unwrap_or_default();
        let cache_dir = resolve_models_dir(&config);

        match create_dense_embedder(&cache_dir, false) {
            Ok(embedder) => *state = EmbedderState::Ready(embedder),
            Err(error) => *state = EmbedderState::Failed(error.to_string()),
        }
    }

    match &mut *state {
        EmbedderState::Ready(embedder) => {
            let mut embeddings = embedder.embed(
                vec![format!("passage: {}", normalize_whitespace(text))],
                None,
            )?;
            embeddings
                .pop()
                .ok_or_else(|| anyhow!("fastembed returned no vectors"))
        }
        EmbedderState::Failed(message) => Err(anyhow!(message.clone())),
        EmbedderState::Uninitialized => Err(anyhow!("embedder did not initialize")),
    }
}

fn create_dense_embedder(cache_dir: &Path, show_download_progress: bool) -> Result<TextEmbedding> {
    let options = TextInitOptions::new(EmbeddingModel::AllMiniLML6V2)
        .with_cache_dir(cache_dir.to_path_buf())
        .with_show_download_progress(show_download_progress);
    TextEmbedding::try_new(options)
}

fn create_reranker(cache_dir: &Path, show_download_progress: bool) -> Result<TextRerank> {
    let options = RerankInitOptions::new(RerankerModel::BGERerankerBase)
        .with_cache_dir(cache_dir.to_path_buf())
        .with_show_download_progress(show_download_progress);
    TextRerank::try_new(options)
}

fn create_splade_embedder(
    cache_dir: &Path,
    show_download_progress: bool,
) -> Result<SparseTextEmbedding> {
    let options = SparseInitOptions::new(SparseModel::SPLADEPPV1)
        .with_cache_dir(cache_dir.to_path_buf())
        .with_show_download_progress(show_download_progress);
    SparseTextEmbedding::try_new(options)
}

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn warn_fastembed_fallback(error: &anyhow::Error) {
    // The runtime deliberately stays usable when model downloads fail, but we only
    // want to surface that fallback once so local batch operations don't get noisy.
    if FASTEMBED_FALLBACK_WARNED.set(()).is_ok() {
        eprintln!(
            "ctx: fastembed is unavailable ({error}). falling back to deterministic hash embeddings until the local models are installed."
        );
    }
}

fn hash_embedding(text: &str, dimensions: usize) -> Vec<f32> {
    // This fallback keeps local indexing functional even before the full model install
    // flow is complete. It behaves like a deterministic bag-of-words projection,
    // so dense ranking stays stable without relying on network access.
    let mut vector = vec![0.0_f32; dimensions];
    for token in tokenize_terms(text) {
        let mut hasher = DefaultHasher::new();
        token.hash(&mut hasher);
        let hash = hasher.finish() as usize;
        let index = hash % dimensions;
        let sign = if (hash / dimensions) % 2 == 0 {
            1.0
        } else {
            -1.0
        };
        vector[index] += sign;
    }

    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in &mut vector {
            *value /= norm;
        }
    }

    vector
}
