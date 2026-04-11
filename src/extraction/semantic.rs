use anyhow::Result;

#[derive(Debug, Clone, Default)]
pub struct Triple {
    pub subject: String,
    pub predicate: String,
    pub object: String,
}

#[derive(Debug, Clone, Default)]
pub struct SemanticExtraction {
    pub summary: String,
    pub facts: Vec<String>,
    pub entities: Vec<String>,
    pub triples: Vec<Triple>,
}

pub async fn extract_semantic(_content: &str, _source_hint: &str) -> Result<SemanticExtraction> {
    anyhow::bail!("semantic extraction is not implemented yet")
}

