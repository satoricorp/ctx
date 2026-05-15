use anyhow::{bail, Result};
use chrono::Local;
use clap::Args;
use std::io::{self, Read};

use crate::cli::scope::{resolve_context_name, ContextSelectArgs};
use crate::cli::theme;
use crate::NotesWriteMode;

#[derive(Debug, Args)]
#[command(
    about = "Write durable text memory into context notes",
    long_about = "Write durable text memory into context notes.\n\nUse this for facts, decisions, summaries, preferences, or reminders that should survive beyond the current chat. Use `ctx add <path>` for files and source material."
)]
pub struct RememberArgs {
    #[command(flatten)]
    pub select: ContextSelectArgs,
    /// Topic name. Defaults to `general`.
    #[arg(long, default_value = "general")]
    pub topic: String,
    /// Read memory text from stdin.
    #[arg(long, conflicts_with = "text")]
    pub stdin: bool,
    /// Text to remember. Quote multi-word text, or use `--stdin`.
    #[arg(value_name = "text")]
    pub text: Option<String>,
}

pub async fn run(args: RememberArgs) -> Result<()> {
    let context = resolve_context_name(&args.select)?;
    let topic = topic_slug(&args.topic)?;
    let content = memory_text(args.stdin, args.text)?;
    let entry = format_memory_entry(&content);
    let path = format!("notes/topics/{topic}.md");

    let outcome = crate::write_notes_file(&context, &path, &entry, NotesWriteMode::Append)?;
    println!(
        "{}",
        theme::success_detail("remembered", context, outcome.path)
    );
    Ok(())
}

fn memory_text(read_stdin: bool, text: Option<String>) -> Result<String> {
    let raw = if read_stdin {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        buffer
    } else {
        text.ok_or_else(|| anyhow::anyhow!("missing text; pass text or use --stdin"))?
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("memory text is empty");
    }
    Ok(trimmed.to_string())
}

fn format_memory_entry(content: &str) -> String {
    let date = Local::now().date_naive();
    format!("## {date}\n\n{content}\n")
}

fn topic_slug(topic: &str) -> Result<String> {
    let topic = topic.trim().strip_suffix(".md").unwrap_or(topic.trim());
    let mut slug = String::new();
    let mut last_was_sep = false;
    let mut has_alnum = false;

    for ch in topic.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_sep = false;
            has_alnum = true;
        } else if matches!(ch, '-' | '_') {
            slug.push(ch);
            last_was_sep = false;
        } else if !last_was_sep {
            slug.push('-');
            last_was_sep = true;
        }
    }

    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() || !has_alnum {
        bail!("topic must contain at least one ASCII letter or number");
    }
    Ok(slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_slug_normalizes_human_topic_names() {
        assert_eq!(topic_slug("Auth").unwrap(), "auth");
        assert_eq!(topic_slug("Auth Policy.md").unwrap(), "auth-policy");
        assert_eq!(topic_slug(" auth/JWT  ").unwrap(), "auth-jwt");
    }

    #[test]
    fn topic_slug_rejects_empty_topics() {
        assert!(topic_slug(" - / ").is_err());
        assert!(topic_slug("__").is_err());
    }

    #[test]
    fn memory_entry_uses_dated_markdown_section() {
        let entry = format_memory_entry("RS256 tokens only.");
        assert!(entry.starts_with("## "));
        assert!(entry.contains("\n\nRS256 tokens only.\n"));
    }
}
