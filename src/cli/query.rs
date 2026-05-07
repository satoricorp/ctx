use anyhow::{bail, Result};
use clap::Args;
use serde::Deserialize;
use std::collections::BTreeSet;

use crate::cli::progress::CliSpinner;
use crate::cli::scope::{resolve_context_name, ContextSelectArgs};
use crate::extraction::json::normalize_llm_json_text;
use crate::retrieval::query::{QueryResult, QueryType};

const COLOR_RESET: &str = "\x1b[0m";
const COLOR_HEADER: &str = "\x1b[33;1m";
const COLOR_SOURCE_BULLET: &str = "\x1b[32m";
const COLOR_SOURCE_TEXT: &str = "\x1b[36m";

#[derive(Debug, Args)]
#[command(
    about = "Search indexed content in a context and synthesize an answer",
    long_about = "Search indexed content in a context and synthesize an answer.\n\nUse -c, --context <name> to query a specific context."
)]
pub struct QueryArgs {
    #[command(flatten)]
    pub select: ContextSelectArgs,
    #[arg(value_name = "text")]
    pub query: String,
    /// Restrict results to one index type.
    #[arg(long = "type", default_value_t = QueryType::All)]
    pub kind: QueryType,
    /// Maximum number of results.
    #[arg(long = "k", default_value_t = 5)]
    pub k: usize,
    /// Print raw retrieved results instead of a synthesized answer.
    #[arg(long = "raw")]
    pub raw: bool,
}

pub async fn run(args: QueryArgs) -> Result<()> {
    let context = resolve_context_name(&args.select)?;
    let results = {
        let spinner = CliSpinner::new("embedding and searching");
        let results = crate::query_context(&context, &args.query, args.kind, args.k).await?;
        spinner.success("embedding complete");
        results
    };
    if results.is_empty() {
        let status = crate::context_status(&context)?;
        eprintln!(
            "no results for context {:?} (chunks {}, procedures {})",
            context, status.counts.chunk_count, status.counts.procedure_count
        );
        return Ok(());
    }

    if args.raw {
        print_raw_results(&results);
        return Ok(());
    }

    let rendered = {
        let spinner = CliSpinner::new("writing answer");
        let rendered = synthesize_answer(&context, &args.query, &results).await?;
        spinner.success("answer complete");
        rendered
    };
    println!("{}", rendered.answer);
    if !rendered.attributions.is_empty() {
        println!();
        println!("{COLOR_HEADER}Sources:{COLOR_RESET}");
        for source in rendered.attributions {
            println!("{COLOR_SOURCE_BULLET}- {COLOR_SOURCE_TEXT}{source}{COLOR_RESET}");
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize, Default)]
struct QueryAnswerPayload {
    answer: String,
    #[serde(default)]
    citations: Vec<usize>,
}

#[derive(Debug, PartialEq, Eq)]
struct RenderedAnswer {
    answer: String,
    attributions: Vec<String>,
}

async fn synthesize_answer(
    context: &str,
    query: &str,
    results: &[QueryResult],
) -> Result<RenderedAnswer> {
    let prompt = build_answer_prompt(context, query, results);
    let raw = crate::models::llm::complete_json(&prompt).await?;
    let normalized = normalize_llm_json_text(&raw);
    let payload: QueryAnswerPayload =
        crate::extraction::json::deserialize_first_json_value(&normalized)?;
    let answer = payload.answer.trim().to_string();
    if answer.is_empty() {
        bail!("query model returned an empty answer");
    }
    Ok(RenderedAnswer {
        answer,
        attributions: collect_attributions(results, &payload.citations),
    })
}

fn print_raw_results(results: &[QueryResult]) {
    for result in results {
        println!("{} {:.3} {}", result.kind, result.score, result.source);
        println!("{}", result.summary);
        println!("{}", result.content);
        println!();
    }
}

fn build_answer_prompt(context: &str, query: &str, results: &[QueryResult]) -> String {
    let mut prompt = String::from(
        "You answer questions about a local ctx context.\n\
         Use only the retrieved results below.\n\
         Return JSON only with this shape:\n\
         {\"answer\":\"...\",\"citations\":[1,2]}\n\
         Rules:\n\
         - answer directly and concisely in plain English\n\
         - if the retrieved results are insufficient, say so plainly\n\
         - citations must be 1-based indices of the retrieved results that support the answer\n\
         - do not quote or reproduce long source passages\n\
         - do not include markdown code fences\n\n",
    );
    prompt.push_str(&format!(
        "Context: {context}\nQuestion: {query}\n\nRetrieved results:\n"
    ));
    for (index, result) in results.iter().enumerate() {
        prompt.push_str(&format!(
            "[{}]\nkind: {}\nsource: {}\nsummary: {}\ncontent:\n{}\n\n",
            index + 1,
            result.kind,
            blank_to_unknown(&result.source),
            truncate_for_prompt(&result.summary, 400),
            truncate_for_prompt(&result.content, 2000),
        ));
    }
    prompt
}

fn collect_attributions(results: &[QueryResult], citations: &[usize]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();

    for citation in citations {
        if let Some(result) = citation.checked_sub(1).and_then(|index| results.get(index)) {
            let label = attribution_label(result);
            if seen.insert(label.clone()) {
                out.push(label);
            }
        }
    }

    if out.is_empty() {
        for result in results {
            let label = attribution_label(result);
            if seen.insert(label.clone()) {
                out.push(label);
            }
            if out.len() >= 5 {
                break;
            }
        }
    }

    out
}

fn attribution_label(result: &QueryResult) -> String {
    let source = result.source.trim();
    if source.is_empty() {
        if result.kind.trim().is_empty() {
            String::from("unknown source")
        } else {
            result.kind.clone()
        }
    } else {
        source.to_string()
    }
}

fn blank_to_unknown(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "unknown"
    } else {
        trimmed
    }
}

fn truncate_for_prompt(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut out = trimmed.chars().take(max_chars).collect::<String>();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_attributions_uses_citations_and_dedupes_sources() {
        let results = vec![
            QueryResult {
                content: String::new(),
                summary: String::new(),
                source: String::from("src/auth.rs"),
                score: 0.9,
                kind: String::from("semantic"),
            },
            QueryResult {
                content: String::new(),
                summary: String::new(),
                source: String::from("src/auth.rs"),
                score: 0.8,
                kind: String::from("semantic"),
            },
            QueryResult {
                content: String::new(),
                summary: String::new(),
                source: String::from("docs/tokens.md"),
                score: 0.7,
                kind: String::from("semantic"),
            },
        ];

        let attributions = collect_attributions(&results, &[1, 2, 3]);
        assert_eq!(
            attributions,
            vec![String::from("src/auth.rs"), String::from("docs/tokens.md")]
        );
    }

    #[test]
    fn collect_attributions_falls_back_to_top_sources() {
        let results = vec![
            QueryResult {
                content: String::new(),
                summary: String::new(),
                source: String::from(""),
                score: 0.9,
                kind: String::from("procedural"),
            },
            QueryResult {
                content: String::new(),
                summary: String::new(),
                source: String::from("docs/deploy.md"),
                score: 0.8,
                kind: String::from("semantic"),
            },
        ];

        let attributions = collect_attributions(&results, &[99]);
        assert_eq!(
            attributions,
            vec![String::from("procedural"), String::from("docs/deploy.md")]
        );
    }

    #[tokio::test]
    async fn synthesize_answer_uses_model_json_and_maps_sources() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let server = crate::test_support::MockOpenAiServer::start();
        let saved_openai = std::env::var("OPENAI_API_KEY").ok();
        let saved_openai_base = std::env::var("CTX_OPENAI_BASE_URL").ok();

        std::env::set_var("OPENAI_API_KEY", "test-openai-key");
        std::env::set_var("CTX_OPENAI_BASE_URL", server.base_url());

        let results = vec![
            QueryResult {
                content: String::from("AuthService uses RS256 tokens."),
                summary: String::from("AuthService uses RS256."),
                source: String::from("src/auth.rs"),
                score: 0.95,
                kind: String::from("semantic"),
            },
            QueryResult {
                content: String::from("Token docs mention issuer validation."),
                summary: String::from("Token validation notes."),
                source: String::from("docs/tokens.md"),
                score: 0.72,
                kind: String::from("semantic"),
            },
        ];

        let rendered = synthesize_answer("demo", "what uses RS256", &results)
            .await
            .expect("synthesize answer");
        assert!(rendered.answer.contains("RS256"));
        assert_eq!(rendered.attributions, vec![String::from("src/auth.rs")]);

        match saved_openai {
            Some(value) => std::env::set_var("OPENAI_API_KEY", value),
            None => std::env::remove_var("OPENAI_API_KEY"),
        }
        match saved_openai_base {
            Some(value) => std::env::set_var("CTX_OPENAI_BASE_URL", value),
            None => std::env::remove_var("CTX_OPENAI_BASE_URL"),
        }
    }
}
