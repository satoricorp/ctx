use anyhow::Result;

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

pub async fn extract_procedural(content: &str, source_hint: &str) -> Result<ProceduralExtraction> {
    let steps = extract_steps(content);
    let task_description = first_meaningful_line(content)
        .unwrap_or_else(|| format!("procedure from {source_hint}"));

    Ok(ProceduralExtraction {
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
    })
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
    if ["failed", "failure", "error", "panic"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return String::from("failure");
    }

    if ["partial", "follow-up", "todo", "next step"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return String::from("partial");
    }

    String::from("success")
}

fn extract_failure_modes(content: &str) -> Vec<String> {
    content
        .split(['.', '\n'])
        .map(str::trim)
        .filter(|sentence| {
            let lower = sentence.to_lowercase();
            !sentence.is_empty()
                && ["error", "failure", "failed", "warning", "issue"]
                    .iter()
                    .any(|needle| lower.contains(needle))
        })
        .map(ToOwned::to_owned)
        .collect()
}

fn first_meaningful_line(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.trim_start_matches('#').trim().to_string())
}
