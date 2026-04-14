use anyhow::Result;
use chrono::Utc;
use futures::stream::{self, StreamExt, TryStreamExt};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use uuid::Uuid;

use crate::artifact::index_path;
use crate::extraction::chunker::{chunk_content, Chunk};
use crate::extraction::semantic::extract_semantic;
use crate::models::embeddings::embed_dense;
use crate::store::get_or_open_env;
use crate::store::schema::{AddOutcome, ChunkRecord, EntityRecord, RelationRecord};

/// Max concurrent per-chunk pipelines (semantic extraction + dense embed). Default `1` (same as
/// the historical sequential loop). Set env `CTX_SEMANTIC_INGEST_CONCURRENCY` to overlap work
/// (e.g. `8` for OpenAI; rate limits may apply). Local Gemma inference is globally serialized
/// inside the LLM layer; higher concurrency can still overlap embedding with the next chunk.
fn semantic_ingest_concurrency() -> usize {
    const ENV: &str = "CTX_SEMANTIC_INGEST_CONCURRENCY";
    std::env::var(ENV)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
        .clamp(1, 256)
}

#[derive(Debug, Clone)]
pub struct SemanticIndexSummary {
    pub summary: String,
    pub chunk_count: usize,
    pub entity_count: usize,
}

pub async fn ingest_semantic_path(ctx_path: &Path, source_path: &Path) -> Result<AddOutcome> {
    let content = fs::read_to_string(source_path)?;
    let summary =
        ingest_semantic_document(ctx_path, &source_path.display().to_string(), &content).await?;
    Ok(AddOutcome {
        chunks_written: summary.chunk_count,
        entities_written: summary.entity_count,
    })
}

pub async fn ingest_semantic_document(
    ctx_path: &Path,
    source_path: &str,
    content: &str,
) -> Result<SemanticIndexSummary> {
    let env = get_or_open_env(&index_path(ctx_path))?;
    let timestamp = Utc::now().timestamp();
    let mut chunks = chunk_content(Path::new(source_path), content);
    if chunks.is_empty() && !content.trim().is_empty() {
        chunks.push(Chunk::new(
            content.trim().to_string(),
            "sentence-window",
            0,
            content.len(),
        ));
    }

    let concurrency = semantic_ingest_concurrency();
    let source_owned = source_path.to_string();

    let mut indexed: Vec<(usize, Chunk, crate::extraction::semantic::SemanticExtraction, Vec<f32>)> =
        stream::iter(chunks.into_iter().enumerate())
            .map(|(idx, chunk)| {
                let source_owned = source_owned.clone();
                async move {
                    let extraction = extract_semantic(&chunk.index_text, &source_owned).await?;
                    let embedding = embed_dense(&chunk.index_text).await?;
                    Ok::<_, anyhow::Error>((idx, chunk, extraction, embedding))
                }
            })
            .buffer_unordered(concurrency)
            .try_collect()
            .await?;

    indexed.sort_by_key(|(idx, _, _, _)| *idx);

    let mut chunk_records = Vec::new();
    let mut entity_records = Vec::new();
    let mut relation_records = Vec::new();
    let mut summaries = Vec::new();

    for (_idx, chunk, extraction, embedding) in indexed {
        let chunk_id = Uuid::new_v4().to_string();
        summaries.push(extraction.summary.clone());

        chunk_records.push(ChunkRecord {
            id: chunk_id.clone(),
            source_path: source_path.to_string(),
            content: chunk.content,
            index_text: chunk.index_text,
            summary: extraction.summary,
            chunk_strategy: chunk.strategy,
            char_start: chunk.char_start,
            char_end: chunk.char_end,
            created_at: timestamp,
            embedding,
        });

        let mut seen_entities = BTreeSet::new();
        for entity in extraction.entities {
            if seen_entities.insert(entity.clone()) {
                entity_records.push(EntityRecord {
                    id: Uuid::new_v4().to_string(),
                    name: entity,
                    kind: String::from("entity"),
                    source_id: chunk_id.clone(),
                    created_at: timestamp,
                });
            }
        }

        for triple in extraction.triples {
            relation_records.push(RelationRecord {
                id: Uuid::new_v4().to_string(),
                subject: triple.subject,
                predicate: triple.predicate,
                object: triple.object,
                source_id: chunk_id.clone(),
                created_at: timestamp,
            });
        }
    }

    let chunk_count = chunk_records.len();
    let entity_count = entity_records.len();
    env.update_state(move |state| {
        for chunk in chunk_records {
            state.chunks.insert(chunk.id.clone(), chunk);
        }
        for entity in entity_records {
            state.entities.insert(entity.id.clone(), entity);
        }
        for relation in relation_records {
            state.relations.insert(relation.id.clone(), relation);
        }
        Ok(())
    })?;

    Ok(SemanticIndexSummary {
        summary: summaries
            .into_iter()
            .find(|summary| !summary.trim().is_empty())
            .unwrap_or_else(|| format!("indexed {}", source_path)),
        chunk_count,
        entity_count,
    })
}
