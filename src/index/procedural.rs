use anyhow::Result;
use chrono::Utc;
use std::fs;
use std::path::Path;
use uuid::Uuid;

use crate::artifact::index_path;
use crate::extraction::procedural::extract_procedural;
use crate::models::embeddings::embed_dense;
use crate::store::get_or_open_env;
use crate::store::schema::{AddOutcome, ProcedureRecord, RecordProcedureInput};

#[derive(Debug, Clone)]
pub struct ProceduralIndexSummary {
    pub id: String,
    pub summary: String,
}

pub async fn ingest_procedural_path(ctx_path: &Path, source_path: &Path) -> Result<AddOutcome> {
    let content = fs::read_to_string(source_path)?;
    ingest_procedural_document(ctx_path, Some(source_path.display().to_string()), &content).await?;
    Ok(AddOutcome {
        chunks_written: 0,
        entities_written: 0,
    })
}

pub async fn ingest_procedural_document(
    ctx_path: &Path,
    source_path: Option<String>,
    content: &str,
) -> Result<ProceduralIndexSummary> {
    let extraction =
        extract_procedural(content, source_path.as_deref().unwrap_or("procedural")).await?;
    let input = RecordProcedureInput {
        task: extraction.task_description,
        steps: extraction.steps,
        outcome: extraction.outcome,
        failure_modes: extraction.failure_modes,
        context: extraction.context,
    };
    record_procedure_structured(ctx_path, source_path, input, Some(extraction.confidence)).await
}

pub async fn record_procedure_structured(
    ctx_path: &Path,
    source_path: Option<String>,
    input: RecordProcedureInput,
    confidence_override: Option<f32>,
) -> Result<ProceduralIndexSummary> {
    let env = get_or_open_env(&index_path(ctx_path))?;
    let id = Uuid::new_v4().to_string();
    let embedding = embed_dense(&input.task).await?;
    let record = ProcedureRecord {
        id: id.clone(),
        source_path,
        task_description: input.task,
        steps: input.steps,
        outcome: input.outcome,
        failure_modes: input.failure_modes,
        context: input.context,
        confidence: confidence_override.unwrap_or(0.85),
        run_count: 1,
        created_at: Utc::now().timestamp(),
        embedding,
    };

    env.update_state(move |state| {
        state.procedures.insert(record.id.clone(), record);
        Ok(())
    })?;

    Ok(ProceduralIndexSummary {
        id,
        summary: String::from("procedural record indexed"),
    })
}
