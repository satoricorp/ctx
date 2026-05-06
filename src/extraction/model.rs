#[derive(Debug, Clone)]
pub enum ExtractionBackend {
    OpenAI { model: String },
}

#[derive(Debug, Clone)]
pub enum EmbeddingBackend {
    OpenAI { model: String },
}
