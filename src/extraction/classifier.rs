use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentLayer {
    Semantic,
    Procedural,
}

impl fmt::Display for ContentLayer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Semantic => "semantic",
            Self::Procedural => "procedural",
        })
    }
}

pub fn classify_content(source_path: &Path, content: &str) -> ContentLayer {
    let file_name = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();

    if matches!(file_name, "AGENTS.md" | "CLAUDE.md" | "CONTEXT.md") {
        return ContentLayer::Procedural;
    }

    let mut bullet_steps = 0usize;
    let mut outcome_signals = 0usize;
    let mut imperative_signals = 0usize;

    for line in content.lines() {
        let trimmed = line.trim_start();
        let numbered = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count() > 0
            && trimmed.contains('.');
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || numbered {
            bullet_steps += 1;
        }

        let lower = trimmed.to_lowercase();
        if ["success", "failure", "failed", "partial", "outcome", "result"]
            .iter()
            .any(|needle| lower.contains(needle))
        {
            outcome_signals += 1;
        }

        if ["run", "open", "install", "verify", "add", "remove", "update", "create"]
            .iter()
            .any(|verb| lower.starts_with(verb))
        {
            imperative_signals += 1;
        }
    }

    if bullet_steps >= 2 && (outcome_signals > 0 || imperative_signals >= 2) {
        return ContentLayer::Procedural;
    }

    ContentLayer::Semantic
}
