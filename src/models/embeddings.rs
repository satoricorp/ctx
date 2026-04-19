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

/// Embed many texts in one blocking call. OpenAI and FastEmbed use batched APIs (fewer HTTP / encode calls).
pub async fn embed_dense_batch(texts: &[String]) -> Result<Vec<Vec<f32>>> {
    let texts = texts.to_vec();
    tokio::task::spawn_blocking(move || embed_dense_sync_batch(&texts)).await?
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
    embed_dense_sync_batch(&[text.to_owned()])?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("embedding batch returned no rows"))
}

fn embedding_batch_size() -> usize {
    const ENV: &str = "CTX_EMBEDDING_BATCH_SIZE";
    std::env::var(ENV)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(256)
        .clamp(1, 2048)
}

fn embed_dense_sync_batch(texts: &[String]) -> Result<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    let config = load_config().unwrap_or_default();
    if let Some(model) = config.embedding_model.strip_prefix("openai:") {
        let model = model.trim();
        if !model.is_empty() {
            return openai_embed_sync_batch(texts, model);
        }
    }

    if texts.iter().all(|t| t.trim().is_empty()) {
        return Ok(vec![vec![0.0; HASH_DIMENSIONS]; texts.len()]);
    }

    if std::env::var("CTX_DISABLE_FASTEMBED").ok().as_deref() == Some("1") {
        return Ok(texts
            .iter()
            .map(|t| hash_embedding(t, HASH_DIMENSIONS))
            .collect());
    }

    match try_fastembed_batch(texts) {
        Ok(embeddings) if embeddings.len() == texts.len() => Ok(embeddings),
        Ok(_) => Err(anyhow!("fastembed batch length mismatch")),
        Err(error) => {
            warn_fastembed_fallback(&error);
            Ok(texts
                .iter()
                .map(|t| hash_embedding(t, HASH_DIMENSIONS))
                .collect())
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

fn openai_embed_sync_batch(texts: &[String], model: &str) -> Result<Vec<Vec<f32>>> {
    let dim = openai_embedding_dimensions(model);
    let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| {
        anyhow!("OPENAI_API_KEY is not set (required for embedding_model openai:{model})")
    })?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .context("failed to build HTTP client for OpenAI embeddings")?;

    let batch_size = embedding_batch_size();
    let mut out: Vec<Vec<f32>> = Vec::with_capacity(texts.len());

    for chunk in texts.chunks(batch_size) {
        if chunk.iter().all(|t| t.trim().is_empty()) {
            for _ in chunk {
                out.push(vec![0.0; dim]);
            }
            continue;
        }

        let inputs: Vec<String> = chunk
            .iter()
            .map(|t| {
                let s = t.trim();
                if s.is_empty() {
                    String::from(" ")
                } else {
                    s.to_string()
                }
            })
            .collect();

        let body = serde_json::json!({
            "model": model,
            "input": inputs,
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

        if status.as_u16() == 200 {
            super::openai_ok::log_openai_success("embeddings");
        }

        let v: serde_json::Value =
            serde_json::from_str(&body_text).context("failed to parse OpenAI embeddings JSON")?;
        let data = v["data"]
            .as_array()
            .ok_or_else(|| anyhow!("unexpected OpenAI embeddings response (missing data array)"))?;

        let mut rows: Vec<Option<Vec<f32>>> = vec![None; chunk.len()];
        for item in data {
            let index = item["index"]
                .as_u64()
                .ok_or_else(|| anyhow!("OpenAI embedding row missing index"))?
                as usize;
            if index >= rows.len() {
                return Err(anyhow!("OpenAI embedding index out of range"));
            }
            let arr = item["embedding"]
                .as_array()
                .ok_or_else(|| anyhow!("OpenAI embedding row missing embedding"))?;
            rows[index] = Some(
                arr.iter()
                    .filter_map(|x| x.as_f64().map(|f| f as f32))
                    .collect(),
            );
        }

        for (i, row) in rows.into_iter().enumerate() {
            let mut vec = row.ok_or_else(|| anyhow!("OpenAI embeddings missing row {}", i))?;
            if chunk.get(i).is_some_and(|t| t.trim().is_empty()) {
                vec = vec![0.0; dim];
            }
            out.push(vec);
        }
    }

    Ok(out)
}

fn try_fastembed_batch(texts: &[String]) -> Result<Vec<Vec<f32>>> {
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
            let batch_cap = embedding_batch_size();
            let mut out: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
            for chunk in texts.chunks(batch_cap) {
                if chunk.iter().all(|t| t.trim().is_empty()) {
                    for _ in chunk {
                        out.push(vec![0.0_f32; HASH_DIMENSIONS]);
                    }
                    continue;
                }

                let inputs: Vec<String> = chunk
                    .iter()
                    .map(|t| {
                        if t.trim().is_empty() {
                            String::from("passage: ")
                        } else {
                            format!("passage: {}", normalize_whitespace(t))
                        }
                    })
                    .collect();

                let embeddings = embedder.embed(inputs, None)?;
                if embeddings.len() != chunk.len() {
                    return Err(anyhow!(
                        "fastembed returned {} vectors for {} inputs",
                        embeddings.len(),
                        chunk.len()
                    ));
                }

                for (row, original) in embeddings.into_iter().zip(chunk.iter()) {
                    if original.trim().is_empty() {
                        out.push(vec![0.0_f32; HASH_DIMENSIONS]);
                    } else {
                        out.push(row);
                    }
                }
            }
            Ok(out)
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
