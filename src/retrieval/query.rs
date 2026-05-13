use anyhow::Result;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

use crate::artifact::index_path;
use crate::install::load_config;
use crate::models::embeddings::{
    cosine_similarity, embed_dense, embed_dense_batch, lexical_overlap,
};
use crate::retrieval::rerank::rerank_documents;
use crate::store::{get_or_open_env, ProcedureRecord, TaskContext};

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

#[derive(Debug, Clone)]
struct SemanticCandidate {
    result: QueryResult,
    rerank_text: String,
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

    let mut candidates: Vec<SemanticCandidate> = state
        .chunks
        .values()
        .cloned()
        .map(|chunk| {
            let dense = cosine_similarity(&query_embedding, &chunk.embedding);
            let lexical = lexical_overlap(query, &chunk.index_text);
            let score = alpha * ((dense + 1.0) / 2.0).clamp(0.0, 1.0)
                + (1.0 - alpha) * lexical.clamp(0.0, 1.0);
            SemanticCandidate {
                rerank_text: chunk.index_text.clone(),
                result: QueryResult {
                    content: chunk.content,
                    summary: chunk.summary,
                    source: chunk.source_path,
                    score,
                    kind: String::from("semantic"),
                },
            }
        })
        .collect();

    candidates.extend(notes_query(query, ctx_path, &query_embedding, alpha).await?);

    candidates.sort_by(|left, right| right.result.score.total_cmp(&left.result.score));
    candidates.truncate(50.min(candidates.len()));

    let rerank_scores = rerank_documents(
        query,
        &candidates
            .iter()
            .map(|candidate| candidate.rerank_text.clone())
            .collect::<Vec<_>>(),
    )
    .await?;

    for (index, rerank_score) in rerank_scores {
        if let Some(candidate) = candidates.get_mut(index) {
            candidate.result.score = (candidate.result.score * 0.8) + (rerank_score * 0.2);
        }
    }

    candidates.sort_by(|left, right| right.result.score.total_cmp(&left.result.score));
    candidates.truncate(k.min(candidates.len()));

    Ok(candidates.into_iter().map(|candidate| candidate.result).collect())
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

async fn notes_query(
    query: &str,
    ctx_path: &Path,
    query_embedding: &[f32],
    alpha: f32,
) -> Result<Vec<SemanticCandidate>> {
    let snapshot = crate::read_notes_summary(ctx_path)?;
    let mut documents: Vec<(String, String)> = Vec::new();

    if let Some(index) = snapshot.index {
        documents.push((String::from("notes/index.md"), index));
    }
    if let Some(summary) = snapshot.summary {
        documents.push((String::from("notes/summary.md"), summary));
    }
    for path in snapshot.topics {
        let content = crate::read_notes_file(ctx_path, &path)?.content;
        documents.push((path, content));
    }

    if documents.is_empty() {
        return Ok(Vec::new());
    }

    let embeddings = embed_dense_batch(
        &documents
            .iter()
            .map(|(_, content)| content.clone())
            .collect::<Vec<_>>(),
    )
    .await?;

    Ok(documents
        .into_iter()
        .zip(embeddings)
        .map(|((path, content), embedding)| {
            let dense = cosine_similarity(query_embedding, &embedding);
            let lexical = lexical_overlap(query, &content);
            let score = alpha * ((dense + 1.0) / 2.0).clamp(0.0, 1.0)
                + (1.0 - alpha) * lexical.clamp(0.0, 1.0);

            SemanticCandidate {
                rerank_text: content.clone(),
                result: QueryResult {
                    summary: summarize_note(&content),
                    content,
                    source: path,
                    score,
                    kind: String::from("notes"),
                },
            }
        })
        .collect())
}

fn summarize_note(content: &str) -> String {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        return trimmed.trim_start_matches('#').trim().to_string();
    }

    String::from("notes")
}
