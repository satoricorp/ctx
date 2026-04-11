use anyhow::Result;
use regex::Regex;
use std::collections::BTreeSet;

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

pub async fn extract_semantic(content: &str, source_hint: &str) -> Result<SemanticExtraction> {
    let facts = extract_facts(content);
    let entities = extract_entities(content);
    let triples = extract_triples(&facts, &entities);
    let summary = summarize(content, source_hint, &facts, &entities);

    Ok(SemanticExtraction {
        summary,
        facts,
        entities,
        triples,
    })
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
