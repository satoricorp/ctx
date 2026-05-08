use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AddOutcome {
    pub chunks_written: usize,
    pub entities_written: usize,
}

/// Per-batch ingestion telemetry reported at the end of `add`/`update` runs. Batched walks
/// accumulate these counters and emit a one-line summary so operators can see at a glance how
/// many files were decoded vs skipped and why, without scanning per-file warning spam.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IngestionSummary {
    pub files_seen: usize,
    pub files_decoded: usize,
    pub files_skipped_denylist: usize,
    pub files_skipped_too_large: usize,
    pub files_skipped_read_error: usize,
    pub files_skipped_decode_error: usize,
    pub files_skipped_encoding_error: usize,
    pub units_written: usize,
    pub chunks_written: usize,
    pub entities_written: usize,
    pub bytes_read: u64,
}

impl IngestionSummary {
    /// Total files that were not decoded, across all skip categories.
    pub fn files_skipped(&self) -> usize {
        self.files_skipped_denylist
            + self.files_skipped_too_large
            + self.files_skipped_read_error
            + self.files_skipped_decode_error
            + self.files_skipped_encoding_error
    }

    /// Human-readable one-line summary for CLI output.
    pub fn format_oneline(&self) -> String {
        format!(
            "decoded {} / skipped {} (denylist {}, too-large {}, read-err {}, decode-err {}, encoding-err {}) | units {} chunks {} entities {}",
            self.files_decoded,
            self.files_skipped(),
            self.files_skipped_denylist,
            self.files_skipped_too_large,
            self.files_skipped_read_error,
            self.files_skipped_decode_error,
            self.files_skipped_encoding_error,
            self.units_written,
            self.chunks_written,
            self.entities_written,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IndexCounts {
    pub chunk_count: usize,
    pub entity_count: usize,
    pub relation_count: usize,
    pub procedure_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextListing {
    pub name: String,
    pub description: Option<String>,
    pub counts: IndexCounts,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextStatus {
    pub name: String,
    pub description: Option<String>,
    pub indexed_count: usize,
    pub dirty_count: usize,
    pub pending_count: usize,
    pub counts: IndexCounts,
    pub extraction_model: String,
    pub embedding_model: String,
    pub splade_enabled: bool,
    pub drifted_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct TaskContext {
    pub language: Option<String>,
    pub framework: Option<String>,
    pub environment: Option<String>,
}

impl TaskContext {
    pub fn from_query(query: &str) -> Self {
        let lower = query.to_lowercase();
        Self {
            language: ["rust", "python", "typescript", "javascript", "go"]
                .iter()
                .find(|needle| lower.contains(**needle))
                .map(|needle| (*needle).to_string()),
            framework: ["axum", "react", "next", "tokio", "django"]
                .iter()
                .find(|needle| lower.contains(**needle))
                .map(|needle| (*needle).to_string()),
            environment: ["docker", "kubernetes", "staging", "production"]
                .iter()
                .find(|needle| lower.contains(**needle))
                .map(|needle| (*needle).to_string()),
        }
    }

    pub fn matches(&self, other: &Self) -> bool {
        fn match_field(left: &Option<String>, right: &Option<String>) -> bool {
            match (left, right) {
                (Some(left), Some(right)) => left == right,
                _ => true,
            }
        }

        match_field(&self.language, &other.language)
            && match_field(&self.framework, &other.framework)
            && match_field(&self.environment, &other.environment)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkRecord {
    pub id: String,
    pub source_path: String,
    pub content: String,
    pub index_text: String,
    pub summary: String,
    pub chunk_strategy: String,
    pub char_start: usize,
    pub char_end: usize,
    pub created_at: i64,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRecord {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub source_id: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationRecord {
    pub id: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub source_id: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcedureRecord {
    pub id: String,
    pub source_path: Option<String>,
    pub task_description: String,
    pub steps: Vec<String>,
    pub outcome: String,
    pub failure_modes: Vec<String>,
    pub context: TaskContext,
    pub confidence: f32,
    pub run_count: u32,
    pub created_at: i64,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordProcedureInput {
    pub task: String,
    pub steps: Vec<String>,
    pub outcome: String,
    pub failure_modes: Vec<String>,
    pub context: TaskContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryCandidate {
    pub id: String,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IndexState {
    pub chunks: HashMap<String, ChunkRecord>,
    pub entities: HashMap<String, EntityRecord>,
    pub relations: HashMap<String, RelationRecord>,
    pub procedures: HashMap<String, ProcedureRecord>,
}

impl IndexState {
    pub fn counts(&self) -> IndexCounts {
        IndexCounts {
            chunk_count: self.chunks.len(),
            entity_count: self.entities.len(),
            relation_count: self.relations.len(),
            procedure_count: self.procedures.len(),
        }
    }

    pub fn latest_update(&self) -> Option<DateTime<Utc>> {
        let latest = self
            .chunks
            .values()
            .map(|chunk| chunk.created_at)
            .chain(
                self.procedures
                    .values()
                    .map(|procedure| procedure.created_at),
            )
            .max()?;

        Utc.timestamp_opt(latest, 0).single()
    }

    pub fn remove_source(&mut self, source_path: &str) {
        let chunk_ids: HashSet<String> = self
            .chunks
            .values()
            .filter(|chunk| chunk.source_path == source_path)
            .map(|chunk| chunk.id.clone())
            .collect();

        self.chunks
            .retain(|_, chunk| chunk.source_path != source_path);
        self.entities
            .retain(|_, entity| !chunk_ids.contains(&entity.source_id));
        self.relations
            .retain(|_, relation| !chunk_ids.contains(&relation.source_id));
        self.procedures.retain(|_, procedure| {
            procedure
                .source_path
                .as_deref()
                .map(|path| path != source_path)
                .unwrap_or(true)
        });
    }
}
