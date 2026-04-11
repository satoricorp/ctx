use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddToolInput {
    pub content: String,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    pub ctx: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryToolInput {
    pub query: String,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub k: Option<usize>,
    pub ctx: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordToolInput {
    pub task: String,
    pub steps: Vec<String>,
    pub outcome: String,
    pub failure_modes: Vec<String>,
    pub ctx: String,
}

