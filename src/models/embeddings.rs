use anyhow::{Result, anyhow};
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use std::collections::{HashSet, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use crate::install::{ensure_base_dirs, load_config, resolve_models_dir};

const HASH_DIMENSIONS: usize = 384;

enum EmbedderState {
    Uninitialized,
    Ready(TextEmbedding),
    Failed(String),
}

static EMBEDDER: OnceLock<Mutex<EmbedderState>> = OnceLock::new();

pub async fn embed_dense(text: &str) -> Result<Vec<f32>> {
    let text = text.to_owned();
    tokio::task::spawn_blocking(move || embed_dense_sync(&text)).await?
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
    if text.trim().is_empty() {
        return Ok(vec![0.0; HASH_DIMENSIONS]);
    }

    if std::env::var("CTX_DISABLE_FASTEMBED").ok().as_deref() == Some("1") {
        return Ok(hash_embedding(text, HASH_DIMENSIONS));
    }

    match try_fastembed(text) {
        Ok(embedding) => Ok(embedding),
        Err(_) => Ok(hash_embedding(text, HASH_DIMENSIONS)),
    }
}

fn try_fastembed(text: &str) -> Result<Vec<f32>> {
    let embedder = EMBEDDER.get_or_init(|| Mutex::new(EmbedderState::Uninitialized));
    let mut state = embedder.lock().expect("embedder lock poisoned");

    if matches!(&*state, EmbedderState::Uninitialized) {
        ensure_base_dirs().ok();
        let config = load_config().unwrap_or_default();
        let options = TextInitOptions::new(EmbeddingModel::AllMiniLML6V2)
            .with_cache_dir(resolve_models_dir(&config))
            .with_show_download_progress(false);

        match TextEmbedding::try_new(options) {
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

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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
        let sign = if (hash / dimensions) % 2 == 0 { 1.0 } else { -1.0 };
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
