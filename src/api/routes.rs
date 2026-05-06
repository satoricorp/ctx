use axum::extract::Query;
use axum::http::StatusCode;
use axum::{
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::extraction::classifier::ContentLayer;
use crate::retrieval::query::QueryType;
use crate::store::schema::RecordProcedureInput;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddRequest {
    pub content: String,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    pub ctx: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    pub query: String,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub k: Option<usize>,
    pub ctx: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordRequest {
    pub task: String,
    pub steps: Vec<String>,
    pub outcome: String,
    pub failure_modes: Vec<String>,
    #[serde(default)]
    pub context: crate::store::schema::TaskContext,
    pub ctx: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddResponse {
    pub chunks_written: usize,
    pub entities_written: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    pub results: Vec<crate::retrieval::query::QueryResult>,
    pub notes: NotesSummaryResponse,
    pub drift_detected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drift_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotesSummaryResponse {
    pub index: Option<String>,
    pub summary: Option<String>,
    pub topics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordResponse {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StatusParams {
    pub ctx: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dirty_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub procedure_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extraction_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub splade_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drifted_files: Option<Vec<String>>,
}

pub fn router() -> Router {
    Router::new()
        .route("/add", post(add_handler))
        .route("/query", post(query_handler))
        .route("/record", post(record_handler))
        .route("/status", get(status_handler))
}

pub async fn add_handler(
    Json(request): Json<AddRequest>,
) -> Result<Json<AddResponse>, (StatusCode, String)> {
    let outcome = crate::add_content_to_context(
        &request.ctx,
        &request.content,
        request.source.as_deref(),
        parse_content_layer(request.r#type.as_deref())?,
    )
    .await
    .map_err(internal_error)?;

    Ok(Json(AddResponse {
        chunks_written: outcome.chunks_written,
        entities_written: outcome.entities_written,
    }))
}

pub async fn query_handler(
    Json(request): Json<QueryRequest>,
) -> Result<Json<QueryResponse>, (StatusCode, String)> {
    let results = crate::query_context(
        &request.ctx,
        &request.query,
        parse_query_type(request.r#type.as_deref())?,
        request.k.unwrap_or(5),
    )
    .await
    .map_err(internal_error)?;

    let ctx_path = crate::open_existing_context(&request.ctx).map_err(internal_error)?;
    let summary = crate::read_notes_summary(&ctx_path).map_err(internal_error)?;
    let drift = crate::drift_state(&ctx_path).map_err(internal_error)?;

    Ok(Json(QueryResponse {
        results,
        notes: NotesSummaryResponse {
            index: summary.index,
            summary: summary.summary,
            topics: summary.topics,
        },
        drift_detected: drift.drift_detected,
        drift_hint: drift.drift_detected.then(|| crate::DRIFT_HINT.to_string()),
    }))
}

pub async fn record_handler(
    Json(request): Json<RecordRequest>,
) -> Result<Json<RecordResponse>, (StatusCode, String)> {
    let id = crate::record_procedure(
        &request.ctx,
        RecordProcedureInput {
            task: request.task,
            steps: request.steps,
            outcome: request.outcome,
            failure_modes: request.failure_modes,
            context: request.context,
        },
    )
    .await
    .map_err(internal_error)?;

    Ok(Json(RecordResponse { id }))
}

pub async fn status_handler(
    Query(params): Query<StatusParams>,
) -> Result<Json<StatusResponse>, (StatusCode, String)> {
    // `/status` does double duty: the bare route is the deployment health check,
    // while `?ctx=name` upgrades it into a cheap context inventory snapshot.
    if let Some(context) = params.ctx {
        let status = crate::context_status(&context).map_err(internal_error)?;
        return Ok(Json(StatusResponse {
            status: String::from("ok"),
            context: Some(status.name),
            indexed_count: Some(status.indexed_count),
            dirty_count: Some(status.dirty_count),
            pending_count: Some(status.pending_count),
            chunk_count: Some(status.counts.chunk_count),
            entity_count: Some(status.counts.entity_count),
            relation_count: Some(status.counts.relation_count),
            procedure_count: Some(status.counts.procedure_count),
            extraction_model: Some(status.extraction_model),
            embedding_model: Some(status.embedding_model),
            splade_enabled: Some(status.splade_enabled),
            drifted_files: Some(status.drifted_files),
        }));
    }

    Ok(Json(StatusResponse {
        status: String::from("ok"),
        context: None,
        indexed_count: None,
        dirty_count: None,
        pending_count: None,
        chunk_count: None,
        entity_count: None,
        relation_count: None,
        procedure_count: None,
        extraction_model: None,
        embedding_model: None,
        splade_enabled: None,
        drifted_files: None,
    }))
}

fn parse_content_layer(value: Option<&str>) -> Result<Option<ContentLayer>, (StatusCode, String)> {
    match value {
        None | Some("") => Ok(None),
        Some("semantic") => Ok(Some(ContentLayer::Semantic)),
        Some("procedural") => Ok(Some(ContentLayer::Procedural)),
        Some(other) => Err((StatusCode::BAD_REQUEST, format!("unknown type {}", other))),
    }
}

fn parse_query_type(value: Option<&str>) -> Result<QueryType, (StatusCode, String)> {
    match value {
        None | Some("") | Some("all") => Ok(QueryType::All),
        Some("semantic") => Ok(QueryType::Semantic),
        Some("procedural") => Ok(QueryType::Procedural),
        Some(other) => Err((StatusCode::BAD_REQUEST, format!("unknown type {}", other))),
    }
}

fn internal_error(error: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn status_without_context_is_health_only() {
        let Json(response) = status_handler(Query(StatusParams { ctx: None }))
            .await
            .expect("health status");

        assert_eq!(response.status, "ok");
        assert!(response.context.is_none());
        assert!(response.chunk_count.is_none());
        assert!(response.extraction_model.is_none());
    }

    #[tokio::test]
    async fn status_with_context_includes_context_snapshot() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let ctx_root = TempDir::new().expect("ctx root");
        let home_root = TempDir::new().expect("home root");
        let saved_openai = std::env::var("OPENAI_API_KEY").ok();

        std::env::set_var("CTX_PATH", ctx_root.path());
        std::env::set_var("HOME", home_root.path());
        std::env::set_var("OPENAI_API_KEY", "test-openai-key");

        crate::init_context("api-status")
            .await
            .expect("init context");
        let Json(response) = status_handler(Query(StatusParams {
            ctx: Some(String::from("api-status")),
        }))
        .await
        .expect("context status");

        assert_eq!(response.status, "ok");
        assert_eq!(response.context.as_deref(), Some("api-status"));
        assert!(response.indexed_count.is_some());
        assert!(response.chunk_count.is_some());
        assert!(response.entity_count.is_some());
        assert!(response.relation_count.is_some());
        assert!(response.procedure_count.is_some());
        assert!(response.extraction_model.is_some());
        assert!(response.embedding_model.is_some());
        assert!(response.splade_enabled.is_some());

        std::env::remove_var("CTX_PATH");
        std::env::remove_var("HOME");
        match saved_openai {
            Some(value) => std::env::set_var("OPENAI_API_KEY", value),
            None => std::env::remove_var("OPENAI_API_KEY"),
        }
    }
}
