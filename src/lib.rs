pub mod api;
pub mod artifact;
pub mod auth;
pub mod cli;
pub mod extraction;
pub mod index;
pub mod install;
pub mod mcp;
pub mod models;
pub mod retrieval;
pub mod store;

use anyhow::Result;
use extraction::classifier::ContentLayer;
use retrieval::query::{QueryResult, QueryType};
use std::path::Path;
use store::schema::{AddOutcome, ContextListing, ContextStatus, RecordProcedureInput};

pub async fn init_context(_name: &str) -> Result<ContextStatus> {
    anyhow::bail!("ctx init is not implemented yet")
}

pub async fn add_to_context(
    _context: &str,
    _path: &Path,
    _layer: Option<ContentLayer>,
) -> Result<AddOutcome> {
    anyhow::bail!("ctx add is not implemented yet")
}

pub async fn query_context(
    _context: &str,
    _query: &str,
    _kind: QueryType,
    _k: usize,
) -> Result<Vec<QueryResult>> {
    anyhow::bail!("ctx query is not implemented yet")
}

pub async fn update_context(_context: &str) -> Result<ContextStatus> {
    anyhow::bail!("ctx update is not implemented yet")
}

pub async fn record_procedure(
    _context: &str,
    _record: RecordProcedureInput,
) -> Result<String> {
    anyhow::bail!("ctx record is not implemented yet")
}

pub fn list_contexts() -> Result<Vec<ContextListing>> {
    anyhow::bail!("ctx list is not implemented yet")
}

pub fn context_status(_context: &str) -> Result<ContextStatus> {
    anyhow::bail!("ctx status is not implemented yet")
}

