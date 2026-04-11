use axum::extract::Query;
use axum::http::StatusCode;
use axum::{Json, Router, routing::{get, post}};
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
    pub chunk_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub procedure_count: Option<usize>,
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

    Ok(Json(QueryResponse { results }))
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
    if let Some(context) = params.ctx {
        let status = crate::context_status(&context).map_err(internal_error)?;
        return Ok(Json(StatusResponse {
            status: String::from("ok"),
            chunk_count: Some(status.counts.chunk_count),
            entity_count: Some(status.counts.entity_count),
            procedure_count: Some(status.counts.procedure_count),
        }));
    }

    Ok(Json(StatusResponse {
        status: String::from("ok"),
        chunk_count: None,
        entity_count: None,
        procedure_count: None,
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
