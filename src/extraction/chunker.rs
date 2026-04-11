#[derive(Debug, Clone)]
pub struct Chunk {
    pub content: String,
    pub index_text: String,
    pub strategy: String,
    pub char_start: usize,
    pub char_end: usize,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SentenceWindowChunk {
    pub sentence: String,
    pub window: String,
    pub sentence_idx: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chunker {
    StructureAware,
    SyntaxAware,
    TurnBased,
    SentenceWindow,
}

