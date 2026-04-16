use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::retrieval::query::QueryResult;
use crate::store::schema::TaskContext;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddToolInput {
    pub content: String,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    pub ctx: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddToolOutput {
    pub chunks_written: usize,
    pub entities_written: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryToolInput {
    pub query: String,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub k: Option<usize>,
    pub ctx: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryToolOutput {
    pub results: Vec<QueryResult>,
    pub aura: AuraSummaryDto,
    pub drift_detected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drift_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct AuraSummaryDto {
    pub index: Option<String>,
    pub aura: Option<String>,
    pub topics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecordToolInput {
    pub task: String,
    pub steps: Vec<String>,
    pub outcome: String,
    pub failure_modes: Vec<String>,
    #[serde(default)]
    pub context: TaskContext,
    pub ctx: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecordToolOutput {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuraReadToolInput {
    pub ctx: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuraReadToolOutput {
    pub path: String,
    pub content: String,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuraWriteToolInput {
    pub ctx: String,
    pub path: String,
    pub content: String,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuraWriteToolOutput {
    pub path: String,
    pub hash: String,
}
