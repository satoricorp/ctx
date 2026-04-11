use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum ExtractionBackend {
    OpenAI { model: String },
    Anthropic { model: String },
    LlamaCpp { model_path: PathBuf },
}

#[derive(Debug, Clone)]
pub enum EmbeddingBackend {
    OpenAI,
    FastEmbed,
}

