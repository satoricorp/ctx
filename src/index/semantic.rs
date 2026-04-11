use anyhow::Result;
use chrono::Utc;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use uuid::Uuid;

use crate::artifact::index_path;
use crate::extraction::chunker::{Chunk, chunk_content};
use crate::extraction::semantic::extract_semantic;
use crate::models::embeddings::embed_dense;
use crate::store::schema::{AddOutcome, ChunkRecord, EntityRecord, RelationRecord};
use crate::store::get_or_open_env;

#[derive(Debug, Clone)]
pub struct SemanticIndexSummary {
    pub summary: String,
    pub chunk_count: usize,
    pub entity_count: usize,
}

pub async fn ingest_semantic_path(ctx_path: &Path, source_path: &Path) -> Result<AddOutcome> {
    let content = fs::read_to_string(source_path)?;
    let summary = ingest_semantic_document(ctx_path, &source_path.display().to_string(), &content).await?;
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
        chunks.push(Chunk::new(content.trim().to_string(), "sentence-window", 0, content.len()));
    }

    let mut chunk_records = Vec::new();
    let mut entity_records = Vec::new();
    let mut relation_records = Vec::new();
    let mut summaries = Vec::new();

    for chunk in chunks {
        let extraction = extract_semantic(&chunk.index_text, source_path).await?;
        let chunk_id = Uuid::new_v4().to_string();
        let embedding = embed_dense(&chunk.index_text).await?;
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
