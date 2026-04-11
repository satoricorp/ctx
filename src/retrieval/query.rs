use anyhow::Result;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, Default, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueryType {
    #[default]
    All,
    Semantic,
    Procedural,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryResult {
    pub content: String,
    pub summary: String,
    pub source: String,
    pub score: f32,
    pub kind: String,
}

pub async fn query(_query: &str, _kind: QueryType, _ctx_path: &Path, _k: usize) -> Result<Vec<QueryResult>> {
    anyhow::bail!("query dispatch is not implemented yet")
}

