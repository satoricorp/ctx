//! Aura update cycle (spec §8.7, §10.6). Runs inside `ctx update`:
//! distills stable aura topic files into a single `## Auto-updated knowledge`
//! H2 section inside `aura/aura.md`. Topic files are left untouched; the
//! existing aura registry entry for `aura/aura.md` is refreshed so the new
//! hash is recorded.

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::future::Future;
use std::path::Path;

use crate::artifact::{aura_path, Manifest};

const AURA_UPDATE_HEADING: &str = "## Auto-updated knowledge";
const LEGACY_PROMOTED_HEADING: &str = "## Promoted knowledge";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuraUpdateOutcome {
    pub candidates_considered: usize,
    pub promoted_paths: Vec<String>,
    pub skipped: Option<SkippedReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkippedReason {
    NoStableTopics,
    BelowMinTopics { found: usize, required: u32 },
    ExtractionUnconfigured,
    LlmError(String),
}

#[derive(Debug, Clone)]
pub struct StableTopic {
    pub path: String,
    pub updated_at: DateTime<Utc>,
    pub content: String,
}

/// Production entry point. Invoked from `update_context`.
pub async fn update_aura(ctx_path: &Path) -> Result<AuraUpdateOutcome> {
    update_aura_internal(ctx_path, extraction_available(), |prompt| async move {
        crate::models::llm::complete_json(&prompt).await
    })
    .await
}

/// Core state machine. `distill_json` receives the rendered prompt and returns
/// the raw JSON body shaped like `{"distilled_markdown": "..."}`. When
/// `extraction_configured` is false, the update short-circuits with
/// `SkippedReason::ExtractionUnconfigured` and the distiller is never invoked.
async fn update_aura_internal<F, Fut>(
    ctx_path: &Path,
    extraction_configured: bool,
    distill_json: F,
) -> Result<AuraUpdateOutcome>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<String>>,
{
    let mut manifest = Manifest::load(ctx_path)?;
    let threshold_days = manifest.config.aura_update_threshold_days;
    let min_topics = manifest.config.aura_update_min_topics;

    let candidates = collect_stable_topics(ctx_path, &manifest, Utc::now(), threshold_days)?;
    let considered = candidates.len();

    if considered == 0 {
        return Ok(AuraUpdateOutcome {
            candidates_considered: 0,
            promoted_paths: Vec::new(),
            skipped: Some(SkippedReason::NoStableTopics),
        });
    }
    if (considered as u32) < min_topics {
        return Ok(AuraUpdateOutcome {
            candidates_considered: considered,
            promoted_paths: Vec::new(),
            skipped: Some(SkippedReason::BelowMinTopics {
                found: considered,
                required: min_topics,
            }),
        });
    }
    if !extraction_configured {
        return Ok(AuraUpdateOutcome {
            candidates_considered: considered,
            promoted_paths: Vec::new(),
            skipped: Some(SkippedReason::ExtractionUnconfigured),
        });
    }

    let aura_md_path = aura_path(ctx_path).join("aura.md");
    let existing = if aura_md_path.exists() {
        fs::read_to_string(&aura_md_path)
            .with_context(|| format!("read {}", aura_md_path.display()))?
    } else {
        String::new()
    };
    let existing_section = extract_auto_updated_section(&existing).unwrap_or_default();

    let prompt = build_prompt(&candidates, &existing_section);

    let raw = match distill_json(prompt).await {
        Ok(raw) => raw,
        Err(err) => {
            return Ok(AuraUpdateOutcome {
                candidates_considered: considered,
                promoted_paths: Vec::new(),
                skipped: Some(SkippedReason::LlmError(format!("{err:#}"))),
            });
        }
    };

    let distilled = match parse_distillation(&raw) {
        Ok(body) => body,
        Err(err) => {
            return Ok(AuraUpdateOutcome {
                candidates_considered: considered,
                promoted_paths: Vec::new(),
                skipped: Some(SkippedReason::LlmError(format!("{err:#}"))),
            });
        }
    };

    let merged = replace_auto_updated_section(&existing, &distilled);
    fs::write(&aura_md_path, &merged)
        .with_context(|| format!("write {}", aura_md_path.display()))?;

    crate::refresh_aura_registry(ctx_path, &mut manifest)?;
    manifest.save(ctx_path)?;

    let promoted_paths = candidates.into_iter().map(|c| c.path).collect();
    Ok(AuraUpdateOutcome {
        candidates_considered: considered,
        promoted_paths,
        skipped: None,
    })
}

/// True when the configured extraction model has a callable backend.
fn extraction_available() -> bool {
    crate::models::llm::should_use_cloud_extraction() || crate::models::llm::should_use_local_llm()
}

fn collect_stable_topics(
    ctx_path: &Path,
    manifest: &Manifest,
    now: DateTime<Utc>,
    threshold_days: u32,
) -> Result<Vec<StableTopic>> {
    let cutoff = now - Duration::days(threshold_days as i64);
    let mut out = Vec::new();
    for entry in &manifest.aura.files {
        if entry.path == "aura/index.md" || entry.path == "aura/aura.md" {
            continue;
        }
        if entry.updated_at > cutoff {
            continue;
        }
        let abs = ctx_path.join(&entry.path);
        if !abs.exists() {
            continue;
        }
        let bytes = fs::read(&abs).with_context(|| format!("read {}", abs.display()))?;
        let current_hash = hash_bytes(&bytes);
        if current_hash != entry.hash {
            continue;
        }
        let content = String::from_utf8(bytes)
            .with_context(|| format!("{} is not valid utf-8", abs.display()))?;
        out.push(StableTopic {
            path: entry.path.clone(),
            updated_at: entry.updated_at,
            content,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn build_prompt(topics: &[StableTopic], existing_section: &str) -> String {
    let mut body = String::new();
    body.push_str(
        "You distill stable aura notes into concise long-term memory.\n\n\
         Produce a JSON object with exactly one key, \"distilled_markdown\", whose value \
         is a markdown string summarizing the stable, shared knowledge across the topic \
         files below. Keep per-source citations inline as \"(source: <path>)\". Do not \
         repeat information already present in the existing auto-updated section.\n\n",
    );
    body.push_str("Existing `## Auto-updated knowledge` section:\n");
    if existing_section.trim().is_empty() {
        body.push_str("(none)\n");
    } else {
        body.push_str(existing_section.trim_end());
        body.push('\n');
    }
    body.push_str("\n---\nTopic files:\n");
    for topic in topics {
        body.push_str(&format!(
            "\n## {} (updated {})\n{}\n",
            topic.path,
            topic.updated_at.to_rfc3339(),
            topic.content.trim_end()
        ));
    }
    body.push_str("\nOutput only the JSON object.\n");
    body
}

#[derive(Debug, Deserialize)]
struct DistillationResponse {
    distilled_markdown: String,
}

fn parse_distillation(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    let start = trimmed
        .find('{')
        .context("distillation response has no JSON object")?;
    let end = trimmed
        .rfind('}')
        .context("distillation response has no JSON object")?;
    if end <= start {
        anyhow::bail!("distillation response has malformed JSON delimiters");
    }
    let slice = &trimmed[start..=end];
    let parsed: DistillationResponse =
        serde_json::from_str(slice).context("parse distillation JSON")?;
    Ok(parsed.distilled_markdown)
}

/// Returns the body of the existing auto-updated section, matching either the
/// current `## Auto-updated knowledge` heading or the legacy
/// `## Promoted knowledge` heading (for in-place migration).
fn extract_auto_updated_section(existing: &str) -> Option<String> {
    let (start, end) = locate_auto_updated_section(existing)?;
    let block = &existing[start..end];
    let mut lines = block.lines();
    lines.next();
    Some(lines.collect::<Vec<_>>().join("\n"))
}

/// Locate the byte range of the current or legacy auto-updated section
/// (heading line through the line before the next H2 or EOF).
fn locate_auto_updated_section(existing: &str) -> Option<(usize, usize)> {
    let is_managed_heading = |s: &str| s == AURA_UPDATE_HEADING || s == LEGACY_PROMOTED_HEADING;

    let mut cursor = 0usize;
    let mut heading_start: Option<usize> = None;
    for line in existing.split_inclusive('\n') {
        let line_start = cursor;
        cursor += line.len();
        let trimmed = line.trim_end_matches(['\n', '\r']);
        match heading_start {
            None => {
                if is_managed_heading(trimmed) {
                    heading_start = Some(line_start);
                }
            }
            Some(_) => {
                if trimmed.starts_with("## ") && !is_managed_heading(trimmed) {
                    return heading_start.map(|s| (s, line_start));
                }
            }
        }
    }
    heading_start.map(|s| (s, existing.len()))
}

fn replace_auto_updated_section(existing: &str, body: &str) -> String {
    let body = body.trim_end_matches('\n').to_string();
    let new_section = format!("{AURA_UPDATE_HEADING}\n\n{body}\n");

    if let Some((start, end)) = locate_auto_updated_section(existing) {
        let mut out = String::with_capacity(existing.len() + new_section.len());
        out.push_str(&existing[..start]);
        out.push_str(&new_section);
        let tail = &existing[end..];
        if !tail.is_empty() {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(tail);
        }
        out
    } else {
        let mut out = existing.to_string();
        if !out.is_empty() && !out.ends_with("\n\n") {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
        }
        out.push_str(&new_section);
        out
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        aura_path as aura_dir, init_context, open_existing_context, test_support, write_aura_file,
        AuraWriteMode,
    };
    use tempfile::TempDir;

    struct PromoEnv {
        _tempdir: TempDir,
        _home_root: TempDir,
        saved_openai: Option<String>,
        saved_anthropic: Option<String>,
    }

    impl Drop for PromoEnv {
        fn drop(&mut self) {
            std::env::remove_var("CTX_DISABLE_FASTEMBED");
            std::env::remove_var("CTX_PATH");
            std::env::remove_var("HOME");
            restore_optional_env("OPENAI_API_KEY", self.saved_openai.as_deref());
            restore_optional_env("ANTHROPIC_API_KEY", self.saved_anthropic.as_deref());
        }
    }

    fn restore_optional_env(key: &str, value: Option<&str>) {
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    fn setup_env() -> PromoEnv {
        let tempdir = TempDir::new().expect("tempdir");
        let home_root = TempDir::new().expect("home root");
        let saved_openai = std::env::var("OPENAI_API_KEY").ok();
        let saved_anthropic = std::env::var("ANTHROPIC_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::set_var("HOME", home_root.path());
        std::env::set_var("CTX_PATH", tempdir.path());
        std::env::set_var("CTX_DISABLE_FASTEMBED", "1");
        PromoEnv {
            _tempdir: tempdir,
            _home_root: home_root,
            saved_openai,
            saved_anthropic,
        }
    }

    fn backdate_topic(manifest: &mut Manifest, path: &str, days: i64) {
        if let Some(entry) = manifest.aura.files.iter_mut().find(|e| e.path == path) {
            entry.updated_at = Utc::now() - Duration::days(days);
        }
    }

    fn fake_distiller(body: &'static str) -> impl FnOnce(String) -> BoxedResultFuture {
        move |_prompt| {
            Box::pin(
                async move { Ok(serde_json::json!({ "distilled_markdown": body }).to_string()) },
            )
        }
    }

    type BoxedResultFuture =
        std::pin::Pin<Box<dyn Future<Output = Result<String>> + Send + 'static>>;

    #[tokio::test]
    async fn aura_update_skipped_without_stable_topics() {
        let _guard = test_support::env_lock().lock().expect("lock");
        let _env = setup_env();
        init_context("promote-empty").await.expect("init");
        let ctx_path = open_existing_context("promote-empty").expect("open");

        let outcome = update_aura_internal(&ctx_path, true, fake_distiller("should not be called"))
            .await
            .expect("run");

        assert_eq!(outcome.candidates_considered, 0);
        assert_eq!(outcome.skipped, Some(SkippedReason::NoStableTopics));
    }

    #[tokio::test]
    async fn aura_update_skipped_below_min_topics() {
        let _guard = test_support::env_lock().lock().expect("lock");
        let _env = setup_env();
        init_context("promote-few").await.expect("init");

        write_aura_file(
            "promote-few",
            "aura/a.md",
            "topic a",
            AuraWriteMode::Replace,
        )
        .expect("write a");
        write_aura_file(
            "promote-few",
            "aura/b.md",
            "topic b",
            AuraWriteMode::Replace,
        )
        .expect("write b");

        let ctx_path = open_existing_context("promote-few").expect("open");
        let mut manifest = Manifest::load(&ctx_path).expect("load");
        backdate_topic(&mut manifest, "aura/topics/a.md", 30);
        backdate_topic(&mut manifest, "aura/topics/b.md", 30);
        manifest.save(&ctx_path).expect("save");

        let outcome = update_aura_internal(&ctx_path, true, fake_distiller("should not be called"))
            .await
            .expect("run");

        assert_eq!(outcome.candidates_considered, 2);
        assert_eq!(
            outcome.skipped,
            Some(SkippedReason::BelowMinTopics {
                found: 2,
                required: 3,
            })
        );
    }

    #[tokio::test]
    async fn aura_update_respects_threshold_days() {
        let _guard = test_support::env_lock().lock().expect("lock");
        let _env = setup_env();
        init_context("promote-fresh").await.expect("init");

        for i in 0..3 {
            write_aura_file(
                "promote-fresh",
                &format!("aura/t{i}.md"),
                &format!("fresh topic {i}"),
                AuraWriteMode::Replace,
            )
            .expect("write");
        }

        let ctx_path = open_existing_context("promote-fresh").expect("open");
        let outcome = update_aura_internal(&ctx_path, true, fake_distiller("should not be called"))
            .await
            .expect("run");

        assert_eq!(outcome.candidates_considered, 0);
        assert_eq!(outcome.skipped, Some(SkippedReason::NoStableTopics));
    }

    #[tokio::test]
    async fn aura_update_replaces_section_idempotently() {
        let _guard = test_support::env_lock().lock().expect("lock");
        let _env = setup_env();
        init_context("promote-idem").await.expect("init");

        for i in 0..3 {
            write_aura_file(
                "promote-idem",
                &format!("aura/t{i}.md"),
                &format!("body {i}"),
                AuraWriteMode::Replace,
            )
            .expect("write");
        }

        let ctx_path = open_existing_context("promote-idem").expect("open");
        let mut manifest = Manifest::load(&ctx_path).expect("load");
        for i in 0..3 {
            backdate_topic(&mut manifest, &format!("aura/topics/t{i}.md"), 30);
        }
        manifest.save(&ctx_path).expect("save");

        let body = "distilled line one\ndistilled line two";
        let outcome = update_aura_internal(&ctx_path, true, fake_distiller(body))
            .await
            .expect("run");
        assert!(outcome.skipped.is_none(), "{:?}", outcome.skipped);
        assert_eq!(outcome.candidates_considered, 3);
        assert_eq!(outcome.promoted_paths.len(), 3);

        let aura_md = aura_dir(&ctx_path).join("aura.md");
        let first = fs::read_to_string(&aura_md).expect("read aura.md");
        assert_eq!(first.matches(AURA_UPDATE_HEADING).count(), 1);
        assert!(first.contains("distilled line one"));

        update_aura_internal(&ctx_path, true, fake_distiller(body))
            .await
            .expect("run 2");
        let second = fs::read_to_string(&aura_md).expect("read aura.md");
        assert_eq!(second.matches(AURA_UPDATE_HEADING).count(), 1);
        assert!(second.contains("distilled line one"));
    }

    #[tokio::test]
    async fn aura_update_preserves_existing_aura_content() {
        let _guard = test_support::env_lock().lock().expect("lock");
        let _env = setup_env();
        init_context("promote-preserve").await.expect("init");

        let ctx_path = open_existing_context("promote-preserve").expect("open");
        let aura_md = aura_dir(&ctx_path).join("aura.md");
        fs::write(&aura_md, "# Aura\n\nhandcrafted line.\n").expect("seed aura.md");

        for i in 0..3 {
            write_aura_file(
                "promote-preserve",
                &format!("aura/t{i}.md"),
                &format!("body {i}"),
                AuraWriteMode::Replace,
            )
            .expect("write");
        }

        let mut manifest = Manifest::load(&ctx_path).expect("load");
        for i in 0..3 {
            backdate_topic(&mut manifest, &format!("aura/topics/t{i}.md"), 30);
        }
        // Also refresh the aura.md entry so our handcrafted hash matches the file on disk.
        let handcrafted_bytes = fs::read(&aura_md).expect("read aura.md");
        if let Some(entry) = manifest
            .aura
            .files
            .iter_mut()
            .find(|e| e.path == "aura/aura.md")
        {
            entry.hash = hash_bytes(&handcrafted_bytes);
        }
        manifest.save(&ctx_path).expect("save");

        let outcome = update_aura_internal(&ctx_path, true, fake_distiller("promoted body"))
            .await
            .expect("run");
        assert!(outcome.skipped.is_none());

        let final_content = fs::read_to_string(&aura_md).expect("read");
        assert!(final_content.contains("handcrafted line."));
        assert!(final_content.contains(AURA_UPDATE_HEADING));
        assert!(final_content.contains("promoted body"));
    }

    #[tokio::test]
    async fn aura_update_skipped_when_extraction_unconfigured() {
        let _guard = test_support::env_lock().lock().expect("lock");
        let _env = setup_env();
        init_context("promote-unconf").await.expect("init");

        for i in 0..3 {
            write_aura_file(
                "promote-unconf",
                &format!("aura/t{i}.md"),
                &format!("body {i}"),
                AuraWriteMode::Replace,
            )
            .expect("write");
        }

        let ctx_path = open_existing_context("promote-unconf").expect("open");
        let mut manifest = Manifest::load(&ctx_path).expect("load");
        for i in 0..3 {
            backdate_topic(&mut manifest, &format!("aura/topics/t{i}.md"), 30);
        }
        manifest.save(&ctx_path).expect("save");

        // No API key is set in setup_env; extraction_model defaults to an OpenAI model.
        let outcome = update_aura(&ctx_path).await.expect("run");
        assert_eq!(outcome.skipped, Some(SkippedReason::ExtractionUnconfigured));
    }

    #[tokio::test]
    async fn aura_update_migrates_legacy_promoted_heading() {
        let _guard = test_support::env_lock().lock().expect("lock");
        let _env = setup_env();
        init_context("promote-legacy").await.expect("init");

        let ctx_path = open_existing_context("promote-legacy").expect("open");
        let aura_md = aura_dir(&ctx_path).join("aura.md");
        fs::write(
            &aura_md,
            format!("# Aura\n\n{LEGACY_PROMOTED_HEADING}\n\nold body\n"),
        )
        .expect("seed aura.md");

        for i in 0..3 {
            write_aura_file(
                "promote-legacy",
                &format!("aura/t{i}.md"),
                &format!("body {i}"),
                AuraWriteMode::Replace,
            )
            .expect("write");
        }

        let mut manifest = Manifest::load(&ctx_path).expect("load");
        for i in 0..3 {
            backdate_topic(&mut manifest, &format!("aura/topics/t{i}.md"), 30);
        }
        let seeded = fs::read(&aura_md).expect("read aura.md");
        if let Some(entry) = manifest
            .aura
            .files
            .iter_mut()
            .find(|e| e.path == "aura/aura.md")
        {
            entry.hash = hash_bytes(&seeded);
        }
        manifest.save(&ctx_path).expect("save");

        let outcome = update_aura_internal(&ctx_path, true, fake_distiller("fresh body"))
            .await
            .expect("run");
        assert!(outcome.skipped.is_none(), "{:?}", outcome.skipped);

        let content = fs::read_to_string(&aura_md).expect("read");
        assert!(
            !content.contains(LEGACY_PROMOTED_HEADING),
            "legacy heading remained: {content}"
        );
        assert_eq!(content.matches(AURA_UPDATE_HEADING).count(), 1);
        assert!(content.contains("fresh body"));
        assert!(!content.contains("old body"));
    }

    #[test]
    fn replace_auto_updated_section_appends_when_absent() {
        let input = "# Aura\n\nfirst line.\n";
        let out = replace_auto_updated_section(input, "body");
        assert!(out.contains("first line."));
        let idx = out.find(AURA_UPDATE_HEADING).expect("heading present");
        assert!(idx > input.find("first line.").unwrap());
        assert!(out.contains("\nbody\n"));
    }

    #[test]
    fn replace_auto_updated_section_replaces_when_present() {
        let input = format!(
            "# Aura\n\npreamble.\n\n{AURA_UPDATE_HEADING}\n\nold body\n\n## Other\n\ntrailing\n"
        );
        let out = replace_auto_updated_section(&input, "new body");
        assert_eq!(out.matches(AURA_UPDATE_HEADING).count(), 1);
        assert!(out.contains("new body"));
        assert!(!out.contains("old body"));
        assert!(out.contains("## Other"));
        assert!(out.contains("trailing"));
    }

    #[test]
    fn replace_auto_updated_section_migrates_legacy_heading() {
        let input = format!(
            "# Aura\n\npreamble.\n\n{LEGACY_PROMOTED_HEADING}\n\nlegacy body\n\n## Other\n\ntrailing\n"
        );
        let out = replace_auto_updated_section(&input, "new body");
        assert!(!out.contains(LEGACY_PROMOTED_HEADING));
        assert_eq!(out.matches(AURA_UPDATE_HEADING).count(), 1);
        assert!(out.contains("new body"));
        assert!(!out.contains("legacy body"));
        assert!(out.contains("## Other"));
    }

    #[test]
    fn parse_distillation_extracts_markdown() {
        let raw = "```json\n{\"distilled_markdown\": \"# hello\\n- one\\n- two\"}\n```";
        let got = parse_distillation(raw).expect("parse");
        assert_eq!(got, "# hello\n- one\n- two");
    }

    #[test]
    fn parse_distillation_errors_on_missing_key() {
        let raw = "{\"other\": \"value\"}";
        assert!(parse_distillation(raw).is_err());
    }
}
