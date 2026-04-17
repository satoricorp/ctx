//! `ctx doctor` — comprehensive artifact health checks.
//!
//! Each check is an async task driven concurrently on the tokio runtime. Checks
//! are grouped into tiers for reporting order, but all start at once and stream
//! their results as they complete via an [`mpsc`] channel. Fix actions (under
//! `--fix`) are queued by the checks and applied serially *after* all checks
//! finish, so manifest writes never race.

use anyhow::{Context, Result};
use std::path::Path;
use tokio::sync::mpsc;
use walkdir::WalkDir;

use crate::artifact::{aura_path, blobs_path, index_path, Manifest};
use crate::{
    canonical_aura_path, drift_state, open_existing_context, rebuild_index, refresh_aura_registry,
    verify_context, IntegrityStatus,
};

/// Ordered reporting tier. All tiers run concurrently; ordering is purely for
/// the final summary (fast checks first, heavy checks last).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum Tier {
    Instant,
    Medium,
    Heavy,
}

impl Tier {
    fn label(self) -> &'static str {
        match self {
            Tier::Instant => "instant",
            Tier::Medium => "medium",
            Tier::Heavy => "heavy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum CheckStatus {
    Ok,
    Warn,
    Fail,
}

impl CheckStatus {
    pub fn label(self) -> &'static str {
        match self {
            CheckStatus::Ok => "ok",
            CheckStatus::Warn => "warn",
            CheckStatus::Fail => "fail",
        }
    }
}

/// Result of one check, streamed back to the CLI as checks complete.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CheckReport {
    pub name: &'static str,
    pub tier: Tier,
    pub status: CheckStatus,
    pub detail: String,
    /// Populated after `--fix` actions run.
    pub fixes_applied: Vec<String>,
}

impl CheckReport {
    fn ok(name: &'static str, tier: Tier, detail: impl Into<String>) -> Self {
        Self {
            name,
            tier,
            status: CheckStatus::Ok,
            detail: detail.into(),
            fixes_applied: Vec::new(),
        }
    }
    fn warn(name: &'static str, tier: Tier, detail: impl Into<String>) -> Self {
        Self {
            name,
            tier,
            status: CheckStatus::Warn,
            detail: detail.into(),
            fixes_applied: Vec::new(),
        }
    }
    fn fail(name: &'static str, tier: Tier, detail: impl Into<String>) -> Self {
        Self {
            name,
            tier,
            status: CheckStatus::Fail,
            detail: detail.into(),
            fixes_applied: Vec::new(),
        }
    }
}

/// Fix actions queued by a check. Applied serially after all checks complete.
#[derive(Debug)]
enum FixAction {
    PruneOrphanBlobs { hashes: Vec<String> },
    RemoveAuraEntries { paths: Vec<String> },
    RegisterAuraFiles,
    RelocateRootTopics { paths: Vec<String> },
    RebuildIndex,
}

/// Whole-artifact doctor report. Sorted by tier, then insertion order within a tier.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DoctorReport {
    pub context: String,
    pub reports: Vec<CheckReport>,
    pub ok: usize,
    pub warn: usize,
    pub fail: usize,
}

impl DoctorReport {
    pub fn has_failures(&self) -> bool {
        self.fail > 0
    }
}

/// Runs every check concurrently, streams each [`CheckReport`] over `tx` as it
/// finishes, then applies queued fixes (when `fix` is `true`) and streams the
/// updated reports. Returns the aggregated [`DoctorReport`].
pub async fn run_doctor(
    context: &str,
    fix: bool,
    tx: mpsc::UnboundedSender<CheckReport>,
) -> Result<DoctorReport> {
    let ctx_path = open_existing_context(context)?;
    let context_owned = context.to_string();

    let mut handles: Vec<tokio::task::JoinHandle<(CheckReport, Option<FixAction>)>> = Vec::new();

    // Tier 1 — instant.
    let p = ctx_path.clone();
    handles.push(tokio::task::spawn_blocking(move || check_manifest_schema(&p)));
    let p = ctx_path.clone();
    handles.push(tokio::task::spawn_blocking(move || check_config_sanity(&p)));
    let p = ctx_path.clone();
    handles.push(tokio::task::spawn_blocking(move || {
        check_aura_missing_files(&p)
    }));
    let p = ctx_path.clone();
    handles.push(tokio::task::spawn_blocking(move || check_aura_unregistered(&p)));
    let p = ctx_path.clone();
    handles.push(tokio::task::spawn_blocking(move || check_index_presence(&p)));

    // Tier 2 — medium.
    let p = ctx_path.clone();
    handles.push(tokio::task::spawn_blocking(move || check_source_drift(&p)));
    let ctx_for_verify = context_owned.clone();
    handles.push(tokio::task::spawn_blocking(move || {
        check_blob_integrity(&ctx_for_verify)
    }));

    let mut reports: Vec<CheckReport> = Vec::new();
    let mut fixes: Vec<FixAction> = Vec::new();

    for h in handles {
        let (report, fix_action) = h.await.context("doctor check panicked")?;
        let _ = tx.send(report.clone());
        reports.push(report);
        if let Some(action) = fix_action {
            fixes.push(action);
        }
    }

    if fix {
        for action in fixes {
            apply_fix(&ctx_path, action, &mut reports, &tx)?;
        }

        let should_rebuild = reports
            .iter()
            .any(|r| r.name == "source_drift" && r.status != CheckStatus::Ok)
            || reports
                .iter()
                .any(|r| r.name == "index_presence" && r.status == CheckStatus::Fail);

        if should_rebuild {
            let report = match rebuild_index(&context_owned).await {
                Ok(status) => CheckReport {
                    name: "index_rebuild",
                    tier: Tier::Heavy,
                    status: CheckStatus::Ok,
                    detail: format!(
                        "rebuilt index ({} chunks, {} entities)",
                        status.counts.chunk_count, status.counts.entity_count
                    ),
                    fixes_applied: vec!["index_rebuild".into()],
                },
                Err(err) => CheckReport {
                    name: "index_rebuild",
                    tier: Tier::Heavy,
                    status: CheckStatus::Fail,
                    detail: format!("rebuild failed: {err:#}"),
                    fixes_applied: Vec::new(),
                },
            };
            let _ = tx.send(report.clone());
            reports.push(report);
        }
    }

    Ok(finalize(context_owned, reports))
}

fn finalize(context: String, mut reports: Vec<CheckReport>) -> DoctorReport {
    reports.sort_by_key(|r| match r.tier {
        Tier::Instant => 0,
        Tier::Medium => 1,
        Tier::Heavy => 2,
    });
    let ok = reports.iter().filter(|r| r.status == CheckStatus::Ok).count();
    let warn = reports
        .iter()
        .filter(|r| r.status == CheckStatus::Warn)
        .count();
    let fail = reports
        .iter()
        .filter(|r| r.status == CheckStatus::Fail)
        .count();
    DoctorReport {
        context,
        reports,
        ok,
        warn,
        fail,
    }
}

// -------------------------------- checks --------------------------------

fn check_manifest_schema(ctx_path: &Path) -> (CheckReport, Option<FixAction>) {
    const NAME: &str = "manifest_schema";
    match Manifest::load(ctx_path) {
        Ok(m) => (
            CheckReport::ok(
                NAME,
                Tier::Instant,
                format!(
                    "version {} ({} sources, {} aura files)",
                    m.version,
                    m.sources.len(),
                    m.aura.files.len()
                ),
            ),
            None,
        ),
        Err(err) => (
            CheckReport::fail(NAME, Tier::Instant, format!("{err:#}")),
            None,
        ),
    }
}

fn check_config_sanity(ctx_path: &Path) -> (CheckReport, Option<FixAction>) {
    const NAME: &str = "config_sanity";
    let manifest = match Manifest::load(ctx_path) {
        Ok(m) => m,
        Err(err) => {
            return (
                CheckReport::fail(NAME, Tier::Instant, format!("manifest load failed: {err:#}")),
                None,
            )
        }
    };
    let mut issues: Vec<String> = Vec::new();
    if manifest.config.aura_update_threshold_days == 0 {
        issues.push("aura_update_threshold_days = 0".into());
    }
    if manifest.config.aura_update_min_topics == 0 {
        issues.push("aura_update_min_topics = 0".into());
    }
    if manifest.config.extraction_model.trim().is_empty() {
        issues.push("extraction_model empty".into());
    }
    if manifest.config.embedding_model.trim().is_empty() {
        issues.push("embedding_model empty".into());
    }

    if issues.is_empty() {
        (
            CheckReport::ok(NAME, Tier::Instant, "all config invariants hold"),
            None,
        )
    } else {
        (
            CheckReport::warn(NAME, Tier::Instant, issues.join("; ")),
            None,
        )
    }
}

fn check_aura_missing_files(ctx_path: &Path) -> (CheckReport, Option<FixAction>) {
    const NAME: &str = "aura_registry_missing";
    let manifest = match Manifest::load(ctx_path) {
        Ok(m) => m,
        Err(err) => {
            return (
                CheckReport::fail(NAME, Tier::Instant, format!("manifest load failed: {err:#}")),
                None,
            )
        }
    };
    let missing: Vec<String> = manifest
        .aura
        .files
        .iter()
        .filter(|entry| !ctx_path.join(&entry.path).exists())
        .map(|entry| entry.path.clone())
        .collect();

    if missing.is_empty() {
        (
            CheckReport::ok(NAME, Tier::Instant, "all registered aura files exist"),
            None,
        )
    } else {
        let detail = format!("{} missing aura entries", missing.len());
        let action = FixAction::RemoveAuraEntries {
            paths: missing.clone(),
        };
        (CheckReport::warn(NAME, Tier::Instant, detail), Some(action))
    }
}

fn check_aura_unregistered(ctx_path: &Path) -> (CheckReport, Option<FixAction>) {
    const NAME: &str = "aura_registry_unregistered";
    let manifest = match Manifest::load(ctx_path) {
        Ok(m) => m,
        Err(err) => {
            return (
                CheckReport::fail(NAME, Tier::Instant, format!("manifest load failed: {err:#}")),
                None,
            )
        }
    };
    let dir = aura_path(ctx_path);
    if !dir.exists() {
        return (
            CheckReport::ok(NAME, Tier::Instant, "no aura directory"),
            None,
        );
    }

    let registered: std::collections::HashSet<String> = manifest
        .aura
        .files
        .iter()
        .map(|e| e.path.clone())
        .collect();

    let mut unregistered: Vec<String> = Vec::new();
    let mut stray_roots: Vec<String> = Vec::new();
    for entry in WalkDir::new(&dir)
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let abs = entry.path();
        if abs.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let rel = abs.strip_prefix(ctx_path).unwrap_or(abs);
        let rel_str = rel
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        if !registered.contains(&rel_str) {
            unregistered.push(rel_str.clone());
        }
        let canonical = canonical_aura_path(&rel_str);
        if canonical != rel_str {
            stray_roots.push(rel_str);
        }
    }

    if unregistered.is_empty() && stray_roots.is_empty() {
        return (
            CheckReport::ok(NAME, Tier::Instant, "aura registry is in sync"),
            None,
        );
    }

    let mut parts: Vec<String> = Vec::new();
    if !unregistered.is_empty() {
        parts.push(format!("{} unregistered file(s)", unregistered.len()));
    }
    if !stray_roots.is_empty() {
        parts.push(format!(
            "{} stray root-level topic(s)",
            stray_roots.len()
        ));
    }

    let action = if !stray_roots.is_empty() {
        FixAction::RelocateRootTopics {
            paths: stray_roots,
        }
    } else {
        FixAction::RegisterAuraFiles
    };

    (
        CheckReport::warn(NAME, Tier::Instant, parts.join(", ")),
        Some(action),
    )
}

fn check_index_presence(ctx_path: &Path) -> (CheckReport, Option<FixAction>) {
    const NAME: &str = "index_presence";
    let idx = index_path(ctx_path);
    if !idx.exists() {
        return (
            CheckReport::fail(NAME, Tier::Instant, "index directory missing"),
            Some(FixAction::RebuildIndex),
        );
    }
    match crate::store::get_or_open_env(&idx) {
        Ok(env) => {
            let counts = env.state().counts();
            (
                CheckReport::ok(
                    NAME,
                    Tier::Instant,
                    format!(
                        "chunks {} entities {} relations {} procedures {}",
                        counts.chunk_count,
                        counts.entity_count,
                        counts.relation_count,
                        counts.procedure_count,
                    ),
                ),
                None,
            )
        }
        Err(err) => (
            CheckReport::fail(NAME, Tier::Instant, format!("index open failed: {err:#}")),
            Some(FixAction::RebuildIndex),
        ),
    }
}

fn check_source_drift(ctx_path: &Path) -> (CheckReport, Option<FixAction>) {
    const NAME: &str = "source_drift";
    match drift_state(ctx_path) {
        Ok(state) if !state.drift_detected => (
            CheckReport::ok(NAME, Tier::Medium, "no drift detected"),
            None,
        ),
        Ok(state) => (
            CheckReport::warn(
                NAME,
                Tier::Medium,
                format!("{} drifted source file(s)", state.drifted_files.len()),
            ),
            None,
        ),
        Err(err) => (
            CheckReport::fail(NAME, Tier::Medium, format!("{err:#}")),
            None,
        ),
    }
}

fn check_blob_integrity(context: &str) -> (CheckReport, Option<FixAction>) {
    const NAME: &str = "blob_integrity";
    let report = match verify_context(context) {
        Ok(r) => r,
        Err(err) => {
            return (
                CheckReport::fail(NAME, Tier::Medium, format!("{err:#}")),
                None,
            )
        }
    };
    let bad: Vec<&_> = report
        .entries
        .iter()
        .filter(|e| !matches!(e.status, IntegrityStatus::Ok))
        .collect();
    let orphan_hashes: Vec<String> = report
        .orphans
        .iter()
        .map(|o| o.blob_hash.trim_start_matches("sha256:").to_string())
        .collect();

    let fix_action = if !orphan_hashes.is_empty() {
        Some(FixAction::PruneOrphanBlobs {
            hashes: orphan_hashes.clone(),
        })
    } else {
        None
    };

    let check_report = if !bad.is_empty() {
        CheckReport::fail(
            NAME,
            Tier::Medium,
            format!(
                "{} tampered/missing blob(s), {} orphan(s)",
                bad.len(),
                orphan_hashes.len()
            ),
        )
    } else if !orphan_hashes.is_empty() {
        CheckReport::warn(
            NAME,
            Tier::Medium,
            format!("{} orphan blob(s)", orphan_hashes.len()),
        )
    } else {
        CheckReport::ok(NAME, Tier::Medium, "all blobs verified")
    };

    (check_report, fix_action)
}

// -------------------------------- fixes --------------------------------

fn apply_fix(
    ctx_path: &Path,
    action: FixAction,
    reports: &mut [CheckReport],
    tx: &mpsc::UnboundedSender<CheckReport>,
) -> Result<()> {
    match action {
        FixAction::PruneOrphanBlobs { hashes } => {
            let dir = blobs_path(ctx_path);
            let mut removed: Vec<String> = Vec::new();
            for hash in hashes {
                let path = dir.join(&hash);
                if path.exists() {
                    std::fs::remove_file(&path)
                        .with_context(|| format!("remove {}", path.display()))?;
                    removed.push(format!("sha256:{hash}"));
                }
            }
            record_fix(reports, tx, "blob_integrity", removed);
        }
        FixAction::RemoveAuraEntries { paths } => {
            let mut manifest = Manifest::load(ctx_path)?;
            let before = manifest.aura.files.len();
            manifest.aura.files.retain(|entry| !paths.contains(&entry.path));
            if manifest.aura.files.len() != before {
                manifest.save(ctx_path)?;
            }
            record_fix(reports, tx, "aura_registry_missing", paths);
        }
        FixAction::RelocateRootTopics { paths } => {
            let mut relocated: Vec<String> = Vec::new();
            for rel in paths {
                let canonical = canonical_aura_path(&rel);
                if canonical == rel {
                    continue;
                }
                let src = ctx_path.join(&rel);
                let dst = ctx_path.join(&canonical);
                if !src.exists() {
                    continue;
                }
                if let Some(parent) = dst.parent() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("create {}", parent.display()))?;
                }
                std::fs::rename(&src, &dst)
                    .with_context(|| format!("rename {} -> {}", src.display(), dst.display()))?;
                relocated.push(format!("{rel} -> {canonical}"));
            }
            let mut manifest = Manifest::load(ctx_path)?;
            refresh_aura_registry(ctx_path, &mut manifest)?;
            manifest.save(ctx_path)?;
            record_fix(reports, tx, "aura_registry_unregistered", relocated);
        }
        FixAction::RegisterAuraFiles => {
            let mut manifest = Manifest::load(ctx_path)?;
            refresh_aura_registry(ctx_path, &mut manifest)?;
            manifest.save(ctx_path)?;
            record_fix(
                reports,
                tx,
                "aura_registry_unregistered",
                vec!["refreshed aura registry".into()],
            );
        }
        FixAction::RebuildIndex => {
            // Handled after the fix loop (heavy tier).
        }
    }
    Ok(())
}

fn record_fix(
    reports: &mut [CheckReport],
    tx: &mpsc::UnboundedSender<CheckReport>,
    name: &str,
    fixes: Vec<String>,
) {
    if fixes.is_empty() {
        return;
    }
    if let Some(report) = reports.iter_mut().find(|r| r.name == name) {
        report.fixes_applied.extend(fixes);
        let _ = tx.send(report.clone());
    }
}

/// Helper returned by `Tier::label` for the CLI printer.
pub fn tier_label(tier: Tier) -> &'static str {
    tier.label()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{init_context, test_support, write_aura_file, AuraWriteMode};
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    struct DocEnv {
        _tempdir: TempDir,
        _home_root: TempDir,
        saved_openai: Option<String>,
        saved_anthropic: Option<String>,
    }

    impl Drop for DocEnv {
        fn drop(&mut self) {
            std::env::remove_var("CTX_DISABLE_FASTEMBED");
            std::env::remove_var("CTX_PATH");
            std::env::remove_var("HOME");
            restore_env("OPENAI_API_KEY", self.saved_openai.as_deref());
            restore_env("ANTHROPIC_API_KEY", self.saved_anthropic.as_deref());
        }
    }

    fn restore_env(key: &str, value: Option<&str>) {
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    fn setup_env() -> DocEnv {
        let tempdir = TempDir::new().expect("tempdir");
        let home_root = TempDir::new().expect("home root");
        let saved_openai = std::env::var("OPENAI_API_KEY").ok();
        let saved_anthropic = std::env::var("ANTHROPIC_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::set_var("HOME", home_root.path());
        std::env::set_var("CTX_PATH", tempdir.path());
        std::env::set_var("CTX_DISABLE_FASTEMBED", "1");
        DocEnv {
            _tempdir: tempdir,
            _home_root: home_root,
            saved_openai,
            saved_anthropic,
        }
    }

    async fn run_collect(context: &str, fix: bool) -> DoctorReport {
        let (tx, mut rx) = mpsc::unbounded_channel::<CheckReport>();
        let drain = tokio::spawn(async move {
            let mut n = 0usize;
            while rx.recv().await.is_some() {
                n += 1;
            }
            n
        });
        let report = run_doctor(context, fix, tx).await.expect("run doctor");
        let streamed = drain.await.expect("drain");
        // At minimum, each check posts one report.
        assert!(streamed >= report.reports.len());
        report
    }

    fn report_for<'a>(report: &'a DoctorReport, name: &str) -> &'a CheckReport {
        report
            .reports
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("missing check {name}"))
    }

    #[tokio::test]
    async fn clean_context_reports_all_ok() {
        let _guard = test_support::env_lock().lock().expect("lock");
        let _env = setup_env();
        init_context("doc-clean").await.expect("init");

        let report = run_collect("doc-clean", false).await;
        assert_eq!(report.fail, 0);
        assert!(report.reports.iter().any(|r| r.name == "manifest_schema"));
        assert_eq!(
            report_for(&report, "manifest_schema").status,
            CheckStatus::Ok
        );
        assert_eq!(
            report_for(&report, "blob_integrity").status,
            CheckStatus::Ok
        );
    }

    #[tokio::test]
    async fn orphan_blob_warns_and_fix_prunes() {
        let _guard = test_support::env_lock().lock().expect("lock");
        let _env = setup_env();
        init_context("doc-orphan").await.expect("init");
        let ctx_path = open_existing_context("doc-orphan").expect("open");

        let blobs = blobs_path(&ctx_path);
        std::fs::create_dir_all(&blobs).expect("mk blobs");
        let orphan = blobs.join("deadbeef");
        std::fs::write(&orphan, b"x").expect("write orphan");

        let before = run_collect("doc-orphan", false).await;
        assert_eq!(
            report_for(&before, "blob_integrity").status,
            CheckStatus::Warn
        );
        assert!(orphan.exists());

        let after = run_collect("doc-orphan", true).await;
        let br = report_for(&after, "blob_integrity");
        assert!(!br.fixes_applied.is_empty(), "expected fix to record");
        assert!(!orphan.exists(), "orphan should be pruned");
    }

    #[tokio::test]
    async fn missing_aura_entry_warns_and_fix_removes() {
        let _guard = test_support::env_lock().lock().expect("lock");
        let _env = setup_env();
        init_context("doc-missing-aura").await.expect("init");
        let ctx_path = open_existing_context("doc-missing-aura").expect("open");

        let mut manifest = Manifest::load(&ctx_path).expect("load");
        manifest.aura.files.push(crate::artifact::manifest::AuraFile {
            path: "aura/topics/ghost.md".into(),
            hash: "sha256:000".into(),
            updated_at: chrono::Utc::now(),
            extra: Default::default(),
        });
        manifest.save(&ctx_path).expect("save");

        let before = run_collect("doc-missing-aura", false).await;
        assert_eq!(
            report_for(&before, "aura_registry_missing").status,
            CheckStatus::Warn
        );

        let after = run_collect("doc-missing-aura", true).await;
        let r = report_for(&after, "aura_registry_missing");
        assert!(r.fixes_applied.iter().any(|s| s.contains("ghost")));
        let m = Manifest::load(&ctx_path).expect("reload");
        assert!(m.aura.files.iter().all(|e| e.path != "aura/topics/ghost.md"));
    }

    #[tokio::test]
    async fn stray_root_topic_warns_and_fix_relocates() {
        let _guard = test_support::env_lock().lock().expect("lock");
        let _env = setup_env();
        init_context("doc-stray").await.expect("init");
        let ctx_path = open_existing_context("doc-stray").expect("open");

        // Write a stray root-level topic directly on disk (bypasses canonicalization).
        let stray = aura_path(&ctx_path).join("stray.md");
        std::fs::write(&stray, "unregistered topic").expect("write stray");

        let before = run_collect("doc-stray", false).await;
        assert_eq!(
            report_for(&before, "aura_registry_unregistered").status,
            CheckStatus::Warn
        );

        let after = run_collect("doc-stray", true).await;
        assert!(!stray.exists(), "root-level topic should be moved");
        assert!(
            ctx_path.join("aura/topics/stray.md").exists(),
            "topic should be relocated under aura/topics/"
        );
        let r = report_for(&after, "aura_registry_unregistered");
        assert!(r.fixes_applied.iter().any(|s| s.contains("aura/stray.md -> aura/topics/stray.md")));
    }

    #[tokio::test]
    async fn missing_index_fails_and_fix_rebuilds() {
        let _guard = test_support::env_lock().lock().expect("lock");
        let _env = setup_env();
        init_context("doc-index").await.expect("init");
        let ctx_path = open_existing_context("doc-index").expect("open");

        let idx = index_path(&ctx_path);
        crate::store::evict_env(&idx);
        std::fs::remove_dir_all(&idx).expect("wipe index");

        let before = run_collect("doc-index", false).await;
        assert_eq!(
            report_for(&before, "index_presence").status,
            CheckStatus::Fail
        );

        let after = run_collect("doc-index", true).await;
        let r = after.reports.iter().find(|r| r.name == "index_rebuild");
        assert!(r.is_some(), "expected index_rebuild report under --fix");
        assert!(index_path(&ctx_path).exists(), "index should be rebuilt");
    }

    #[tokio::test]
    async fn concurrency_emits_one_report_per_check() {
        let _guard = test_support::env_lock().lock().expect("lock");
        let _env = setup_env();
        init_context("doc-concurrency").await.expect("init");
        // Seed one topic so registry path has content.
        write_aura_file(
            "doc-concurrency",
            "aura/note.md",
            "note",
            AuraWriteMode::Replace,
        )
        .expect("write topic");

        let (tx, mut rx) = mpsc::unbounded_channel::<CheckReport>();
        let streamed = tokio::spawn(async move {
            let mut names: Vec<String> = Vec::new();
            while let Some(r) = rx.recv().await {
                names.push(r.name.to_string());
            }
            names
        });
        let report = run_doctor("doc-concurrency", false, tx)
            .await
            .expect("run");
        let names = streamed.await.expect("drain");

        // Every check name appears at least once in the stream.
        for r in &report.reports {
            assert!(
                names.iter().any(|n| n == r.name),
                "check {} never streamed",
                r.name
            );
        }
        assert_eq!(report.reports.len(), 7);
    }
}

