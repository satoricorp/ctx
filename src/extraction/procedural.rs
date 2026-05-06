use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::sync::OnceLock;

use super::json::{deserialize_first_json_value, extract_json_object};
use crate::store::schema::TaskContext;

#[derive(Debug, Clone, Default)]
pub struct ProceduralExtraction {
    pub task_description: String,
    pub steps: Vec<String>,
    pub outcome: String,
    pub failure_modes: Vec<String>,
    pub context: TaskContext,
    pub confidence: f32,
}

#[derive(Debug, Deserialize)]
struct ProceduralLlmOutput {
    task_description: String,
    #[serde(default)]
    steps: Vec<String>,
    outcome: String,
    #[serde(default)]
    failure_modes: Vec<String>,
    #[serde(default)]
    context: TaskContext,
    confidence: Option<f32>,
}

pub async fn extract_procedural(content: &str, source_hint: &str) -> Result<ProceduralExtraction> {
    if crate::models::llm::should_use_cloud_extraction() {
        match extract_procedural_with_llm(content).await {
            Ok(extraction) => return Ok(extraction),
            Err(error) => warn_cloud_extraction_fallback_once(&error),
        }
    } else if crate::models::llm::should_warn_missing_extraction_api_key() {
        crate::models::llm::warn_missing_extraction_api_key_once();
    }

    Ok(extract_procedural_rules(content, source_hint))
}

async fn extract_procedural_with_llm(content: &str) -> Result<ProceduralExtraction> {
    let prompt = format!(
        "extract a procedure record:\n{{\n  \"task_description\": \"one sentence\",\n  \"steps\": [\"ordered\", \"concrete\", \"actions\"],\n  \"outcome\": \"success | failure | partial\",\n  \"failure_modes\": [],\n  \"context\": {{ \"language\": null, \"framework\": null, \"environment\": null }},\n  \"confidence\": 0.0\n}}\nrespond as JSON only.\n\ncontent:\n{content}"
    );
    let raw = crate::models::llm::complete_json(&prompt).await?;
    parse_procedural_llm_output(&raw)
}

fn parse_procedural_llm_output(raw: &str) -> Result<ProceduralExtraction> {
    let json = extract_json_object(raw)?;
    let parsed: ProceduralLlmOutput = deserialize_first_json_value(json)
        .map_err(|error| anyhow!("failed to parse procedural extraction JSON: {error}"))?;

    let task_description = parsed.task_description.trim().to_string();
    if task_description.is_empty() {
        return Err(anyhow!(
            "procedural extraction returned an empty task description"
        ));
    }

    let steps = parsed
        .steps
        .into_iter()
        .map(|step| step.trim().to_string())
        .filter(|step| !step.is_empty())
        .collect::<Vec<_>>();
    let outcome = normalize_outcome(&parsed.outcome);
    if outcome.is_empty() {
        return Err(anyhow!("procedural extraction returned an invalid outcome"));
    }

    Ok(ProceduralExtraction {
        task_description,
        steps,
        outcome,
        failure_modes: parsed
            .failure_modes
            .into_iter()
            .map(|mode| mode.trim().to_string())
            .filter(|mode| !mode.is_empty())
            .collect(),
        context: parsed.context,
        confidence: parsed.confidence.unwrap_or(0.7).clamp(0.0, 1.0),
    })
}

fn warn_cloud_extraction_fallback_once(error: &anyhow::Error) {
    static WARNED: OnceLock<()> = OnceLock::new();
    if WARNED.set(()).is_ok() {
        eprintln!(
            "ctx: cloud procedural extraction failed ({error}). falling back to heuristic procedural extraction."
        );
    }
}

fn extract_procedural_rules(content: &str, source_hint: &str) -> ProceduralExtraction {
    let steps = extract_steps(content);
    let task_description =
        first_meaningful_line(content).unwrap_or_else(|| format!("procedure from {source_hint}"));

    ProceduralExtraction {
        task_description,
        steps: if steps.is_empty() {
            vec![String::from("review the source content")]
        } else {
            steps
        },
        outcome: infer_outcome(content),
        failure_modes: extract_failure_modes(content),
        context: TaskContext::from_query(content),
        confidence: if content.contains('\n') { 0.78 } else { 0.62 },
    }
}

fn extract_steps(content: &str) -> Vec<String> {
    let mut steps: Vec<String> = content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                return Some(trimmed[2..].trim().to_string());
            }

            let digits = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
            if digits > 0 {
                let rest = trimmed[digits..].trim_start_matches('.').trim();
                if !rest.is_empty() {
                    return Some(rest.to_string());
                }
            }

            None
        })
        .collect();

    if !steps.is_empty() {
        return steps;
    }

    steps = content
        .split(['.', '\n'])
        .map(str::trim)
        .filter(|sentence| sentence.len() > 15)
        .take(6)
        .map(ToOwned::to_owned)
        .collect();

    steps
}

fn infer_outcome(content: &str) -> String {
    let lower = content.to_lowercase();
    if lower.contains("success") || lower.contains("passed") || lower.contains("completed") {
        return String::from("success");
    }
    if lower.contains("fail") || lower.contains("error") || lower.contains("rollback") {
        return String::from("failure");
    }
    String::from("partial")
}

fn extract_failure_modes(content: &str) -> Vec<String> {
    content
        .split(['.', '\n'])
        .map(str::trim)
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("fail") || lower.contains("error") || lower.contains("timeout")
        })
        .take(5)
        .map(ToOwned::to_owned)
        .collect()
}

fn first_meaningful_line(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| line.len() > 10)
        .map(ToOwned::to_owned)
}

fn normalize_outcome(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "success" => String::from("success"),
        "failure" => String::from("failure"),
        "partial" => String::from("partial"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_procedural_json_with_prefix_suffix() {
        let raw = r#"prefix {"task_description":"Deploy auth service","steps":["Run tests","Deploy"],"outcome":"success","failure_modes":[],"context":{"language":"rust","framework":"axum","environment":"staging"},"confidence":0.92} suffix"#;
        let parsed = parse_procedural_llm_output(raw).expect("procedural json");
        assert_eq!(parsed.task_description, "Deploy auth service");
        assert_eq!(parsed.outcome, "success");
        assert_eq!(parsed.steps.len(), 2);
        assert_eq!(parsed.context.framework.as_deref(), Some("axum"));
    }
}
