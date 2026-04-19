//! Helpers for recovering structured output from LLM responses.

use anyhow::{anyhow, Result};
use serde::de::DeserializeOwned;

/// Parse the first JSON value from `text`, ignoring trailing data after a complete value.
/// Models sometimes append prose, markdown fences, or duplicate fragments after valid JSON.
pub fn deserialize_first_json_value<T: DeserializeOwned>(text: &str) -> Result<T> {
    let s = strip_unicode_bom(text.trim_start());
    let mut iter = serde_json::Deserializer::from_str(s).into_iter::<T>();
    match iter.next() {
        Some(Ok(value)) => Ok(value),
        Some(Err(e)) => Err(e.into()),
        None => Err(anyhow!("model output contained no JSON value")),
    }
}

fn strip_unicode_bom(s: &str) -> &str {
    s.strip_prefix('\u{FEFF}').unwrap_or(s)
}

/// Strip leading UTF-8 BOM, trim, and remove common markdown ``` / ```json fences around JSON.
pub fn normalize_llm_json_text(raw: &str) -> String {
    let s = strip_unicode_bom(raw.trim()).trim();
    if let Some(rest) = s.strip_prefix("```") {
        let rest = rest.trim_start();
        let rest = rest
            .strip_prefix("json")
            .or_else(|| rest.strip_prefix("JSON"))
            .unwrap_or(rest)
            .trim_start();
        if let Some(end) = rest.rfind("```") {
            return rest[..end].trim().to_string();
        }
        return rest.to_string();
    }
    s.to_string()
}

/// Find the first top-level JSON object in `raw` by matching `{` … `}` with nesting.
/// Ignores braces inside double-quoted strings (with `\` escapes). Using `rfind('}')` is wrong
/// when the model emits a `}` before the JSON (e.g. preamble or assistant prose), which used to
/// panic with `begin <= end` when slicing.
pub fn extract_json_object(raw: &str) -> Result<&str> {
    let start = raw
        .find('{')
        .ok_or_else(|| anyhow!("model output did not contain a JSON object start"))?;

    let bytes = raw.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;

    for i in start..raw.len() {
        let b = bytes[i];

        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }

        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(&raw[start..=i]);
                }
            }
            _ => {}
        }
    }

    Err(anyhow!(
        "model output did not contain a balanced JSON object (opened at byte {start}, no matching `}}`)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Obj {
        a: i32,
    }

    #[test]
    fn deserialize_first_value_ignores_trailing_prose() {
        let v: Obj = deserialize_first_json_value(r#"{"a":1} trailing commentary"#).expect("parse");
        assert_eq!(v, Obj { a: 1 });
    }

    #[test]
    fn deserialize_first_value_ignores_second_concatenated_object() {
        let v: Obj = deserialize_first_json_value(r#"{"a":1}{"a":2}"#).expect("parse first only");
        assert_eq!(v, Obj { a: 1 });
    }
}
