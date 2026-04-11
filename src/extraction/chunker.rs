use std::path::Path;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub content: String,
    pub index_text: String,
    pub strategy: String,
    pub char_start: usize,
    pub char_end: usize,
    pub session_id: Option<String>,
}

impl Chunk {
    pub fn new(
        content: impl Into<String>,
        strategy: impl Into<String>,
        char_start: usize,
        char_end: usize,
    ) -> Self {
        let content = content.into();
        Self {
            index_text: content.clone(),
            content,
            strategy: strategy.into(),
            char_start,
            char_end,
            session_id: None,
        }
    }

    pub fn with_session(
        content: impl Into<String>,
        strategy: impl Into<String>,
        char_start: usize,
        char_end: usize,
        session_id: impl Into<String>,
    ) -> Self {
        let mut chunk = Self::new(content, strategy, char_start, char_end);
        chunk.session_id = Some(session_id.into());
        chunk
    }
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

impl Chunker {
    pub fn detect(source_path: &Path, content: &str) -> Self {
        match source_path.extension().and_then(|ext| ext.to_str()) {
            Some("md") | Some("mdx") | Some("rst") => {
                if content.lines().any(|line| line.starts_with('#')) {
                    Self::StructureAware
                } else {
                    Self::SentenceWindow
                }
            }
            Some("txt") => Self::SentenceWindow,
            Some("ts") | Some("tsx") | Some("js") | Some("jsx") | Some("rs")
            | Some("py") | Some("go") | Some("java") | Some("c") | Some("cpp")
            | Some("cs") | Some("rb") | Some("swift") | Some("kt") | Some("scala") => {
                Self::SyntaxAware
            }
            Some("jsonl") => {
                if content.contains("\"role\"") || content.contains("\"speaker\"") {
                    Self::TurnBased
                } else {
                    Self::SentenceWindow
                }
            }
            _ => Self::SentenceWindow,
        }
    }
}

pub fn chunk_content(source_path: &Path, content: &str) -> Vec<Chunk> {
    match Chunker::detect(source_path, content) {
        Chunker::StructureAware => chunk_structure(content),
        Chunker::SyntaxAware => chunk_syntax(
            content,
            source_path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or_default(),
        ),
        Chunker::TurnBased => chunk_turns(content),
        Chunker::SentenceWindow => chunk_sentence_window(content, 2)
            .into_iter()
            .map(|window| Chunk::new(window.window, "sentence-window", 0, 0))
            .collect(),
    }
}

pub fn token_count(content: &str) -> usize {
    content.chars().count() / 4
}

pub fn chunk_structure(content: &str) -> Vec<Chunk> {
    let sections = split_on_headings(content);
    if sections.is_empty() {
        return chunk_sentence_window(content, 2)
            .into_iter()
            .map(|window| Chunk::new(window.window, "sentence-window", 0, 0))
            .collect();
    }

    let mut chunks = Vec::new();
    for (heading, body, start, end) in sections {
        if token_count(&body) <= 400 {
            chunks.push(Chunk::new(
                format!("{}\n{}", heading, body),
                "structure-aware",
                start,
                end,
            ));
            continue;
        }

        for paragraph in body.split("\n\n") {
            let paragraph = paragraph.trim();
            if !paragraph.is_empty() {
                chunks.push(Chunk::new(
                    format!("{}\n{}", heading, paragraph),
                    "structure-aware",
                    start,
                    end,
                ));
            }
        }
    }

    chunks
}

pub fn chunk_syntax(content: &str, ext: &str) -> Vec<Chunk> {
    let boundaries = detect_syntax_boundaries(content, ext);
    if boundaries.is_empty() {
        return chunk_sentence_window(content, 2)
            .into_iter()
            .map(|window| Chunk::new(window.window, "sentence-window", 0, 0))
            .collect();
    }

    let mut chunks = Vec::new();
    for (start, end) in boundaries {
        let body = &content[start..end];
        if token_count(body) <= 600 {
            chunks.push(Chunk::new(
                body.trim().to_string(),
                "syntax-aware",
                start,
                end,
            ));
            continue;
        }

        let (signature, rest) = split_signature(body);
        for paragraph in rest.split("\n\n") {
            let paragraph = paragraph.trim();
            if !paragraph.is_empty() {
                chunks.push(Chunk::new(
                    format!("{}\n// ...\n{}", signature, paragraph),
                    "syntax-aware",
                    start,
                    end,
                ));
            }
        }
    }

    chunks
}

pub fn chunk_turns(content: &str) -> Vec<Chunk> {
    let turns = parse_turns(content);
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_session = String::new();

    for turn in turns {
        if turn.is_session_boundary {
            if !current.trim().is_empty() {
                chunks.push(Chunk::with_session(
                    current.trim().to_string(),
                    "turn-based",
                    0,
                    0,
                    current_session.clone(),
                ));
            }
            current.clear();
            current_session = turn.session_id.unwrap_or_default();
            continue;
        }

        if token_count(&current) + token_count(&turn.text) > 600 && !current.trim().is_empty() {
            chunks.push(Chunk::with_session(
                current.trim().to_string(),
                "turn-based",
                0,
                0,
                current_session.clone(),
            ));
            current = turn.text;
            continue;
        }

        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(&turn.text);
    }

    if !current.trim().is_empty() {
        chunks.push(Chunk::with_session(
            current.trim().to_string(),
            "turn-based",
            0,
            0,
            current_session,
        ));
    }

    chunks
}

pub fn chunk_sentence_window(content: &str, window_size: usize) -> Vec<SentenceWindowChunk> {
    let sentences: Vec<String> = split_sentences(content)
        .into_iter()
        .filter(|sentence| sentence.trim().len() >= 20)
        .collect();

    sentences
        .iter()
        .enumerate()
        .map(|(index, sentence)| {
            let start = index.saturating_sub(window_size);
            let end = (index + window_size + 1).min(sentences.len());
            SentenceWindowChunk {
                sentence: sentence.clone(),
                window: sentences[start..end].join(" "),
                sentence_idx: index,
            }
        })
        .collect()
}

fn split_on_headings(content: &str) -> Vec<(String, String, usize, usize)> {
    let mut sections = Vec::new();
    let mut current_heading = String::from("document");
    let mut current_body = String::new();
    let mut section_start = 0usize;
    let mut offset = 0usize;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            if !current_body.trim().is_empty() {
                sections.push((
                    current_heading.clone(),
                    current_body.trim().to_string(),
                    section_start,
                    offset,
                ));
            }

            current_heading = trimmed.to_string();
            current_body.clear();
            section_start = offset;
        } else {
            current_body.push_str(line);
            current_body.push('\n');
        }

        offset += line.len() + 1;
    }

    if !current_body.trim().is_empty() {
        sections.push((
            current_heading,
            current_body.trim().to_string(),
            section_start,
            content.len(),
        ));
    }

    sections
}

fn detect_syntax_boundaries(content: &str, ext: &str) -> Vec<(usize, usize)> {
    let lines: Vec<&str> = content.lines().collect();
    let mut offsets = Vec::with_capacity(lines.len());
    let mut byte_offset = 0usize;
    for line in &lines {
        offsets.push(byte_offset);
        byte_offset += line.len() + 1;
    }

    let mut starts = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if is_boundary(line.trim_start(), ext) {
            starts.push(offsets[find_comment_start(&lines, index)]);
        }
    }

    starts.sort_unstable();
    starts.dedup();

    starts
        .iter()
        .enumerate()
        .filter_map(|(index, start)| {
            let end = starts.get(index + 1).copied().unwrap_or(content.len());
            (*start < end).then_some((*start, end))
        })
        .collect()
}

fn find_comment_start(lines: &[&str], start_index: usize) -> usize {
    let mut cursor = start_index;
    let lower_bound = start_index.saturating_sub(10);
    while cursor > lower_bound {
        let previous = lines[cursor - 1].trim_start();
        if previous.starts_with("///")
            || previous.starts_with("//!")
            || previous.starts_with("#")
            || previous.starts_with("//")
        {
            cursor -= 1;
        } else {
            break;
        }
    }
    cursor
}

fn is_boundary(line: &str, ext: &str) -> bool {
    match ext {
        "rs" => ["pub fn ", "fn ", "impl ", "struct ", "enum ", "mod "]
            .iter()
            .any(|pattern| line.starts_with(pattern)),
        "ts" | "tsx" | "js" | "jsx" => {
            ["function ", "class ", "export function ", "export default "]
                .iter()
                .any(|pattern| line.starts_with(pattern))
        }
        "py" => ["def ", "async def ", "class "]
            .iter()
            .any(|pattern| line.starts_with(pattern)),
        "go" => line.starts_with("func "),
        "java" | "kt" | "scala" => ["class ", "fun ", "object "]
            .iter()
            .any(|pattern| line.starts_with(pattern)),
        _ => false,
    }
}

fn split_signature(body: &str) -> (&str, &str) {
    if let Some((signature, rest)) = body.split_once('\n') {
        (signature.trim(), rest)
    } else {
        (body.trim(), "")
    }
}

fn split_sentences(content: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();

    for ch in content.chars() {
        current.push(ch);
        if matches!(ch, '.' | '!' | '?' | '\n') {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                sentences.push(trimmed.to_string());
            }
            current.clear();
        }
    }

    if !current.trim().is_empty() {
        sentences.push(current.trim().to_string());
    }

    sentences
}

#[derive(Debug)]
struct ParsedTurn {
    text: String,
    session_id: Option<String>,
    is_session_boundary: bool,
}

fn parse_turns(content: &str) -> Vec<ParsedTurn> {
    let mut turns = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.to_lowercase().starts_with("session:") {
            turns.push(ParsedTurn {
                text: String::new(),
                session_id: Some(trimmed.split(':').nth(1).unwrap_or_default().trim().to_string()),
                is_session_boundary: true,
            });
            continue;
        }

        turns.push(ParsedTurn {
            text: trimmed.to_string(),
            session_id: None,
            is_session_boundary: false,
        });
    }
    turns
}
