use anyhow::{anyhow, Context, Result};
use std::collections::HashSet;

use crate::install::load_config;

pub async fn embed_dense(text: &str) -> Result<Vec<f32>> {
    let text = text.to_owned();
    tokio::task::spawn_blocking(move || embed_dense_sync(&text)).await?
}

pub async fn embed_dense_batch(texts: &[String]) -> Result<Vec<Vec<f32>>> {
    let texts = texts.to_vec();
    tokio::task::spawn_blocking(move || embed_dense_sync_batch(&texts)).await?
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
    let model = config
        .embedding_model
        .strip_prefix("openai:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "unsupported embedding_model {}; only openai:* is supported",
                config.embedding_model
            )
        })?;

    openai_embed_sync_batch(texts, model)
}

fn openai_embedding_dimensions(model: &str) -> usize {
    let lower = model.to_ascii_lowercase();
    if lower.contains("3-large") {
        return 3072;
    }
    if lower.contains("3-small") {
        return 1536;
    }
    1536
}

fn openai_base_url() -> String {
    std::env::var("CTX_OPENAI_BASE_URL")
        .ok()
        .map(|value| value.trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| String::from("https://api.openai.com/v1"))
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
            .post(format!("{}/embeddings", openai_base_url()))
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
