use anyhow::Result;

use crate::store::schema::TaskContext;

#[derive(Debug, Clone, Default)]
pub struct ProceduralExtraction {
    pub task_description: String,
    pub steps: Vec<String>,
    pub outcome: String,
    pub failure_modes: Vec<String>,
    pub context: TaskContext,
    pub confidence: f32,
}

pub async fn extract_procedural(_content: &str, _source_hint: &str) -> Result<ProceduralExtraction> {
    anyhow::bail!("procedural extraction is not implemented yet")
}

