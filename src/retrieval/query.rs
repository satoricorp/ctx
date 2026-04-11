use anyhow::Result;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

use crate::artifact::index_path;
use crate::install::load_config;
use crate::models::embeddings::{cosine_similarity, embed_dense, lexical_overlap};
use crate::retrieval::rerank::rerank_documents;
use crate::store::{get_or_open_env, ChunkRecord, ProcedureRecord, TaskContext};

#[derive(Debug, Clone, Copy, Default, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueryType {
    #[default]
    All,
    Semantic,
    Procedural,
}

impl fmt::Display for QueryType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::All => "all",
            Self::Semantic => "semantic",
            Self::Procedural => "procedural",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct QueryResult {
    pub content: String,
    pub summary: String,
    pub source: String,
    pub score: f32,
    pub kind: String,
}

pub async fn unified_query(query: &str, ctx_path: &Path, k: usize) -> Result<Vec<QueryResult>> {
    let task_context = TaskContext::from_query(query);
    let (semantic_results, procedural_results) = tokio::join!(
        semantic_query(query, ctx_path, k),
        procedural_query(query, &task_context, ctx_path, k),
    );

    let mut results = semantic_results?;
    results.extend(procedural_results?);
    results.sort_by(|left, right| right.score.total_cmp(&left.score));
    results.truncate(k.min(results.len()));
    Ok(results)
}

pub async fn query(
    query: &str,
    kind: QueryType,
    ctx_path: &Path,
    k: usize,
) -> Result<Vec<QueryResult>> {
    match kind {
        QueryType::All => unified_query(query, ctx_path, k).await,
        QueryType::Semantic => semantic_query(query, ctx_path, k).await,
        QueryType::Procedural => {
            procedural_query(query, &TaskContext::from_query(query), ctx_path, k).await
        }
    }
}

async fn semantic_query(query: &str, ctx_path: &Path, k: usize) -> Result<Vec<QueryResult>> {
    let env = get_or_open_env(&index_path(ctx_path))?;
    let state = env.state();
    let config = load_config().unwrap_or_default();
    let alpha = crate::install::effective_alpha(&config);
    let query_embedding = embed_dense(query).await?;

    let mut candidates: Vec<(ChunkRecord, f32)> = state
        .chunks
        .values()
        .cloned()
        .map(|chunk| {
            let dense = cosine_similarity(&query_embedding, &chunk.embedding);
            let lexical = lexical_overlap(query, &chunk.index_text);
            let score = alpha * ((dense + 1.0) / 2.0).clamp(0.0, 1.0)
                + (1.0 - alpha) * lexical.clamp(0.0, 1.0);
            (chunk, score)
        })
        .collect();

    candidates.sort_by(|left, right| right.1.total_cmp(&left.1));
    candidates.truncate(50.min(candidates.len()));

    let rerank_scores = rerank_documents(
        query,
        &candidates
            .iter()
            .map(|(chunk, _)| chunk.index_text.clone())
            .collect::<Vec<_>>(),
    )
    .await?;

    for (index, rerank_score) in rerank_scores {
        if let Some((_, score)) = candidates.get_mut(index) {
            *score = (*score * 0.8) + (rerank_score * 0.2);
        }
    }

    candidates.sort_by(|left, right| right.1.total_cmp(&left.1));
    candidates.truncate(k.min(candidates.len()));

    Ok(candidates
        .into_iter()
        .map(|(chunk, score)| QueryResult {
            content: chunk.content,
            summary: chunk.summary,
            source: chunk.source_path,
            score,
            kind: String::from("semantic"),
        })
        .collect())
}

async fn procedural_query(
    task: &str,
    task_context: &TaskContext,
    ctx_path: &Path,
    k: usize,
) -> Result<Vec<QueryResult>> {
    let env = get_or_open_env(&index_path(ctx_path))?;
    let state = env.state();
    let task_embedding = embed_dense(task).await?;

    let mut ranked: Vec<(&ProcedureRecord, f32)> = state
        .procedures
        .values()
        .filter(|record| record.context.matches(task_context))
        .map(|record| {
            let dense = cosine_similarity(&task_embedding, &record.embedding);
            let lexical = lexical_overlap(task, &record.task_description);
            let score = ((dense + 1.0) / 2.0).clamp(0.0, 1.0)
                * record.confidence
                * recency_weight(record.created_at)
                + lexical * 0.15;
            (record, score)
        })
        .collect();

    ranked.sort_by(|left, right| right.1.total_cmp(&left.1));
    ranked.truncate(k.min(ranked.len()));

    Ok(ranked
        .into_iter()
        .map(|(record, score)| QueryResult {
            content: format!("steps: {}", record.steps.join(" -> ")),
            summary: record.task_description.clone(),
            source: record
                .source_path
                .clone()
                .unwrap_or_else(|| String::from("procedural")),
            score,
            kind: String::from("procedural"),
        })
        .collect())
}

fn recency_weight(created_at: i64) -> f32 {
    let days = (chrono::Utc::now().timestamp() - created_at) as f32 / 86_400.0;
    (-days / 130.0_f32).exp()
}
