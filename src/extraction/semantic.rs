use anyhow::{anyhow, bail, Result};
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::OnceLock;

use super::json::{deserialize_first_json_value, extract_json_object, normalize_llm_json_text};

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

const MAX_JSON_WRAPPER_DEPTH: u8 = 8;

#[derive(Debug, Deserialize)]
struct SemanticLlmOutput {
    summary: String,
    #[serde(default)]
    facts: Vec<String>,
    #[serde(default)]
    entities: Vec<String>,
    #[serde(default)]
    triples: Vec<(String, String, String)>,
}

pub async fn extract_semantic(content: &str, source_hint: &str) -> Result<SemanticExtraction> {
    if crate::models::llm::should_use_cloud_extraction() {
        match extract_semantic_with_llm(content, source_hint).await {
            Ok(extraction) => return Ok(extraction),
            Err(error) => warn_cloud_extraction_fallback_once(&error),
        }
    } else if crate::models::llm::should_warn_missing_extraction_api_key() {
        crate::models::llm::warn_missing_extraction_api_key_once();
    }

    Ok(extract_semantic_rules(content, source_hint))
}

async fn extract_semantic_with_llm(content: &str, source_hint: &str) -> Result<SemanticExtraction> {
    let prompt = format!(
        "extract from this content:\n\n1. a self-contained summary under 200 tokens. include entity names explicitly.\n2. extract ALL atomic facts. no limit. each must be readable as a standalone statement.\n3. named entities: systems, technologies, decisions, people, concepts.\n4. relationships as triples: [subject, predicate, object]\n\nrespond as JSON only:\n{{\n  \"summary\": \"...\",\n  \"facts\": [\"...\", \"...\"],\n  \"entities\": [\"...\", \"...\"],\n  \"triples\": [[\"AuthService\", \"uses\", \"RS256\"]]\n}}\n\nsource: {source_hint}\ncontent:\n{content}"
    );
    let raw = crate::models::llm::complete_json(&prompt).await?;
    parse_semantic_llm_output(&raw)
}

fn parse_semantic_llm_output(raw: &str) -> Result<SemanticExtraction> {
    let parsed: SemanticLlmOutput = parse_semantic_llm_output_inner(raw, 0)
        .map_err(|error| anyhow!("failed to parse semantic extraction JSON: {error}"))?;

    let mut entities = BTreeSet::new();
    for entity in parsed.entities {
        let entity = entity.trim();
        if !entity.is_empty() {
            entities.insert(entity.to_string());
        }
    }

    let triples = parsed
        .triples
        .into_iter()
        .filter_map(|(subject, predicate, object)| {
            let subject = subject.trim();
            let predicate = predicate.trim();
            let object = object.trim();
            if subject.is_empty() || predicate.is_empty() || object.is_empty() {
                return None;
            }
            Some(Triple {
                subject: subject.to_string(),
                predicate: predicate.to_lowercase(),
                object: object.to_string(),
            })
        })
        .collect::<Vec<_>>();

    let facts = parsed
        .facts
        .into_iter()
        .map(|fact| fact.trim().to_string())
        .filter(|fact| !fact.is_empty())
        .collect::<Vec<_>>();
    let summary = parsed.summary.trim().to_string();
    if summary.is_empty() {
        return Err(anyhow!("semantic extraction returned an empty summary"));
    }

    Ok(SemanticExtraction {
        summary,
        facts,
        entities: entities.into_iter().collect(),
        triples,
    })
}

fn parse_semantic_llm_output_inner(raw: &str, depth: u8) -> Result<SemanticLlmOutput> {
    if depth > MAX_JSON_WRAPPER_DEPTH {
        bail!("semantic extraction JSON nested too deeply");
    }
    let normalized = normalize_llm_json_text(raw);
    let mut candidates: Vec<String> = vec![normalized.clone()];
    if let Ok(ext) = extract_json_object(&normalized) {
        let ext = ext.to_string();
        if ext != normalized {
            candidates.push(ext);
        }
    }

    let mut last_err: Option<anyhow::Error> = None;
    for cand in candidates {
        match try_semantic_llm_output_candidate(&cand, depth) {
            Ok(parsed) => return Ok(parsed),
            Err(e) => last_err = Some(e),
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow!("could not parse semantic extraction JSON")))
}

fn try_semantic_llm_output_candidate(cand: &str, depth: u8) -> Result<SemanticLlmOutput> {
    let trimmed = cand.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("empty JSON candidate"));
    }
    // Parse the first complete JSON value, then map into our struct (handles trailing prose).
    let value: Value = deserialize_first_json_value(trimmed)?;
    semantic_llm_output_from_value(value, depth)
}

fn semantic_llm_output_from_value(v: Value, depth: u8) -> Result<SemanticLlmOutput> {
    match v {
        Value::Object(_) => serde_json::from_value(v).map_err(|e| anyhow!("{e}")),
        Value::Array(arr) => match arr.len() {
            1 => semantic_llm_output_from_value(
                arr.into_iter().next().expect("length checked"),
                depth,
            ),
            _ => Err(anyhow!(
                "expected one JSON object or a single-element array for semantic extraction"
            )),
        },
        Value::String(s) => parse_semantic_llm_output_inner(&s, depth + 1),
        _ => Err(anyhow!(
            "expected a JSON object for semantic extraction (got non-object JSON)"
        )),
    }
}

fn warn_cloud_extraction_fallback_once(error: &anyhow::Error) {
    static WARNED: OnceLock<()> = OnceLock::new();
    if WARNED.set(()).is_ok() {
        eprintln!(
            "ctx: cloud extraction failed ({error}). falling back to heuristic semantic extraction."
        );
    }
}

fn extract_semantic_rules(content: &str, source_hint: &str) -> SemanticExtraction {
    let facts = extract_facts(content);
    let entities = extract_entities(content);
    let triples = extract_triples(&facts, &entities);
    let summary = summarize(content, source_hint, &facts, &entities);

    SemanticExtraction {
        summary,
        facts,
        entities,
        triples,
    }
}

fn extract_facts(content: &str) -> Vec<String> {
    let mut facts: Vec<String> = content
        .split(['.', '\n'])
        .map(str::trim)
        .filter(|sentence| sentence.len() >= 20)
        .take(16)
        .map(ToOwned::to_owned)
        .collect();

    if facts.is_empty() && !content.trim().is_empty() {
        facts.push(content.trim().chars().take(220).collect());
    }

    facts
}

fn extract_entities(content: &str) -> Vec<String> {
    let title_case = Regex::new(r"\b[A-Z][A-Za-z0-9_]{2,}\b").expect("valid entity regex");
    let code_symbol = Regex::new(
        r"\b(?:fn|struct|enum|class|trait|impl|mod|function|interface)\s+([A-Za-z_][A-Za-z0-9_]*)",
    )
    .expect("valid symbol regex");

    let mut entities = BTreeSet::new();
    for capture in title_case.find_iter(content) {
        entities.insert(capture.as_str().to_string());
    }
    for capture in code_symbol.captures_iter(content) {
        if let Some(symbol) = capture.get(1) {
            entities.insert(symbol.as_str().to_string());
        }
    }

    entities.into_iter().take(24).collect()
}

fn extract_triples(facts: &[String], entities: &[String]) -> Vec<Triple> {
    let mut triples = Vec::new();
    let triple_regex = Regex::new(
        r"(?i)\b([A-Za-z_][A-Za-z0-9_:-]+)\s+(uses|handles|stores|writes|reads|supports|calls|wraps|creates|returns|contains|runs)\s+([A-Za-z_][A-Za-z0-9_:-]+)",
    )
    .expect("valid triple regex");

    for fact in facts {
        for capture in triple_regex.captures_iter(fact) {
            triples.push(Triple {
                subject: capture[1].to_string(),
                predicate: capture[2].to_lowercase(),
                object: capture[3].to_string(),
            });
        }
    }

    if triples.is_empty() && entities.len() >= 2 {
        triples.push(Triple {
            subject: entities[0].clone(),
            predicate: String::from("mentions"),
            object: entities[1].clone(),
        });
    }

    triples
}

fn summarize(content: &str, source_hint: &str, facts: &[String], entities: &[String]) -> String {
    let lead = facts
        .first()
        .cloned()
        .unwrap_or_else(|| content.trim().chars().take(120).collect::<String>());

    if entities.is_empty() {
        return format!("{} ({})", compact(&lead, 180), source_hint);
    }

    format!(
        "{} entities: {}",
        compact(&lead, 140),
        compact(&entities.join(", "), 60)
    )
}

fn compact(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.trim().to_string();
    }

    let trimmed: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}…", trimmed.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::json::deserialize_first_json_value;

    #[test]
    fn parses_semantic_json_with_prefix_suffix() {
        let raw = r#"preamble {"summary":"AuthService uses RS256.","facts":["AuthService uses RS256 for tokens."],"entities":["AuthService","RS256"],"triples":[["AuthService","uses","RS256"]]} trailing"#;
        let parsed = parse_semantic_llm_output(raw).expect("semantic json");
        assert_eq!(parsed.summary, "AuthService uses RS256.");
        assert_eq!(parsed.entities.len(), 2);
        assert_eq!(parsed.triples[0].predicate, "uses");
    }

    #[test]
    fn parses_json_when_closing_brace_appears_before_opening_in_preamble() {
        let raw = r#"Sure. } here is JSON: {"summary":"ok","facts":[],"entities":[],"triples":[]}"#;
        let json = extract_json_object(raw).expect("extract");
        assert!(json.starts_with('{') && json.ends_with('}'));
        let parsed = parse_semantic_llm_output(raw).expect("semantic json");
        assert_eq!(parsed.summary, "ok");
    }

    #[test]
    fn brace_in_json_string_does_not_end_object_early() {
        let raw = r#"{"summary":"uses } in text","facts":[],"entities":[],"triples":[]}"#;
        let parsed = parse_semantic_llm_output(raw).expect("semantic json");
        assert_eq!(parsed.summary, "uses } in text");
    }

    #[test]
    fn parses_semantic_json_with_trailing_chars_inside_extracted_slice() {
        // `from_str` rejects trailing non-whitespace; stream parse accepts the first value.
        let json = r#"{"summary":"ok","facts":[],"entities":[],"triples":[]} tail"#;
        let parsed: SemanticLlmOutput = deserialize_first_json_value(json).expect("first value");
        assert_eq!(parsed.summary, "ok");
    }

    #[test]
    fn parses_semantic_json_fenced_with_trailing_prose() {
        let raw = "```json\n{\"summary\":\"hi\",\"facts\":[],\"entities\":[],\"triples\":[]}\n```\nThanks!";
        let parsed = parse_semantic_llm_output(raw).expect("parse");
        assert_eq!(parsed.summary, "hi");
    }
}
