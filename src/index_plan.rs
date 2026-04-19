//! Pre-flight work planning for interactive add/update (file list + skip reasons).

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::artifact::Manifest;
use crate::extraction::classifier::ContentLayer;
use crate::{classify_path, parse_layer, probe_index_file, IndexFilePlan, SkipCheck};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkItem {
    pub root: PathBuf,
    pub rel: PathBuf,
    pub abs: PathBuf,
    pub layer: Option<ContentLayer>,
    pub with_content: bool,
    #[serde(default)]
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanWalkStats {
    pub files_seen: usize,
    pub skipped_denylist: usize,
    pub skipped_too_large: usize,
    pub skipped_already_indexed: usize,
    pub skipped_stat_error: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkPlan {
    pub items: Vec<WorkItem>,
    pub stats: PlanWalkStats,
    /// Semantic vs procedural counts for rough time estimates.
    pub semantic_work_items: usize,
    pub procedural_work_items: usize,
}

impl WorkPlan {
    pub fn work_bytes(&self) -> u64 {
        self.items.iter().map(|i| i.size_bytes).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Plan a directory add (same walk rules as [`crate::add_to_context_with_verbosity`]).
pub fn plan_add_directory(
    ctx_path: &Path,
    root: &Path,
    layer: Option<ContentLayer>,
    with_content: bool,
) -> Result<WorkPlan> {
    let root_str = root.display().to_string();
    let mut plan = WorkPlan::default();

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let entry_path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }
        match classify_path(entry_path) {
            SkipCheck::Keep => {}
            SkipCheck::DeniedExtension => {
                plan.stats.files_seen += 1;
                plan.stats.skipped_denylist += 1;
                continue;
            }
            SkipCheck::InfrastructureDir | SkipCheck::JunkFile => continue,
        }

        plan.stats.files_seen += 1;
        let rel = entry_path
            .strip_prefix(root)
            .unwrap_or(entry_path)
            .to_path_buf();

        match probe_index_file(
            ctx_path,
            &root_str,
            &rel,
            entry_path,
            with_content,
        ) {
            Ok(IndexFilePlan::SkipAlreadyIndexed) => {
                plan.stats.skipped_already_indexed += 1;
            }
            Ok(IndexFilePlan::SkipTooLarge { .. }) => {
                plan.stats.skipped_too_large += 1;
            }
            Ok(IndexFilePlan::NeedsWork { size }) => {
                count_layer(layer, &mut plan);
                plan.items.push(WorkItem {
                    root: root.to_path_buf(),
                    rel,
                    abs: entry_path.to_path_buf(),
                    layer,
                    with_content,
                    size_bytes: size,
                });
            }
            Err(_) => {
                plan.stats.skipped_stat_error += 1;
            }
        }
    }

    Ok(plan)
}

fn count_layer(layer: Option<ContentLayer>, plan: &mut WorkPlan) {
    match layer {
        Some(ContentLayer::Procedural) => plan.procedural_work_items += 1,
        Some(ContentLayer::Semantic) | None => plan.semantic_work_items += 1,
    }
}

/// Plan adding a single file (`root` = parent dir, `rel` = file name).
pub fn plan_add_single_file(
    ctx_path: &Path,
    root: &Path,
    rel: &Path,
    abs: &Path,
    layer: Option<ContentLayer>,
    with_content: bool,
) -> Result<WorkPlan> {
    let root_str = root.display().to_string();
    let mut plan = WorkPlan::default();
    plan.stats.files_seen = 1;

    match probe_index_file(ctx_path, &root_str, rel, abs, with_content) {
        Ok(IndexFilePlan::SkipAlreadyIndexed) => {
            plan.stats.skipped_already_indexed = 1;
        }
        Ok(IndexFilePlan::SkipTooLarge { .. }) => {
            plan.stats.skipped_too_large = 1;
        }
        Ok(IndexFilePlan::NeedsWork { size }) => {
            count_layer(layer, &mut plan);
            plan.items.push(WorkItem {
                root: root.to_path_buf(),
                rel: rel.to_path_buf(),
                abs: abs.to_path_buf(),
                layer,
                with_content,
                size_bytes: size,
            });
        }
        Err(_) => {
            plan.stats.skipped_stat_error = 1;
        }
    }
    Ok(plan)
}

/// Plan `ctx update`: manifest sources that exist on disk and need re-indexing.
pub fn plan_manifest_update(ctx_path: &Path, with_content: bool) -> Result<WorkPlan> {
    let manifest = Manifest::load(ctx_path)?;
    let mut plan = WorkPlan::default();

    let mut targets: Vec<(PathBuf, PathBuf, Option<ContentLayer>)> = Vec::new();
    let mut seen: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    for source in &manifest.sources {
        let root = PathBuf::from(&source.root);
        for entry in &source.files {
            let source_path = entry.effective_source_path().to_string();
            if !seen.insert((source.root.clone(), source_path.clone())) {
                continue;
            }
            let rel = PathBuf::from(&source_path);
            let layer = parse_layer(&entry.r#type).ok();
            targets.push((root.clone(), rel, layer));
        }
    }

    for (root, source_rel, layer) in targets {
        let abs = root.join(&source_rel);
        if !abs.exists() {
            continue;
        }
        plan.stats.files_seen += 1;
        let root_str = root.display().to_string();

        match probe_index_file(
            ctx_path,
            &root_str,
            &source_rel,
            &abs,
            with_content,
        ) {
            Ok(IndexFilePlan::SkipAlreadyIndexed) => {
                plan.stats.skipped_already_indexed += 1;
            }
            Ok(IndexFilePlan::SkipTooLarge { .. }) => {
                plan.stats.skipped_too_large += 1;
            }
            Ok(IndexFilePlan::NeedsWork { size }) => {
                match layer {
                    Some(ContentLayer::Procedural) => plan.procedural_work_items += 1,
                    Some(ContentLayer::Semantic) | None => plan.semantic_work_items += 1,
                }
                plan.items.push(WorkItem {
                    root,
                    rel: source_rel,
                    abs,
                    layer,
                    with_content,
                    size_bytes: size,
                });
            }
            Err(_) => {
                plan.stats.skipped_stat_error += 1;
            }
        }
    }

    Ok(plan)
}

pub fn describe_plan_line(plan: &WorkPlan) -> String {
    format!(
        "plan: {} file(s) to index, {} already up to date, {} denylist, {} too large, {} stat err | ~{:.1} MiB",
        plan.items.len(),
        plan.stats.skipped_already_indexed,
        plan.stats.skipped_denylist,
        plan.stats.skipped_too_large,
        plan.stats.skipped_stat_error,
        plan.work_bytes() as f64 / (1024.0 * 1024.0)
    )
}

// --- duration estimate + telemetry ---

const DEFAULT_SEMANTIC_SECS_PER_MIB: f64 = 45.0;
const DEFAULT_PROCEDURAL_SECS_PER_FILE: f64 = 8.0;
const DEFAULT_FIXED_PER_FILE: f64 = 2.0;

/// Rough seconds estimate (not a guarantee). Uses EMA when available.
pub fn estimate_seconds(plan: &WorkPlan) -> f64 {
    let mib = plan.work_bytes() as f64 / (1024.0 * 1024.0);
    let stats = load_indexing_stats();
    let sem_secs = stats
        .map(|s| s.ema_secs_per_mib_semantic)
        .unwrap_or(DEFAULT_SEMANTIC_SECS_PER_MIB);
    let mut t = mib * sem_secs;
    t += plan.procedural_work_items as f64 * DEFAULT_PROCEDURAL_SECS_PER_FILE;
    t += plan.items.len() as f64 * DEFAULT_FIXED_PER_FILE;
    t.max(1.0)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IndexingStatsFile {
    #[serde(default)]
    pub ema_secs_per_mib_semantic: f64,
    #[serde(default)]
    pub sample_count: u64,
}

fn stats_path() -> PathBuf {
    dirs::home_dir()
        .expect("home dir")
        .join(".ctx")
        .join("indexing-stats.json")
}

pub fn load_indexing_stats() -> Option<IndexingStatsFile> {
    let path = stats_path();
    let data = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Blend observed run into EMA (`alpha` weight on new sample).
pub fn record_indexing_observation(plan: &WorkPlan, elapsed_secs: f64, alpha: f64) {
    if plan.semantic_work_items == 0 || plan.work_bytes() == 0 {
        return;
    }
    let mib = plan.work_bytes() as f64 / (1024.0 * 1024.0);
    if mib < 0.001 {
        return;
    }
    let sample = elapsed_secs / mib;
    let alpha = alpha.clamp(0.05, 0.5);
    let mut cur = load_indexing_stats().unwrap_or_default();
    if cur.sample_count == 0 {
        cur.ema_secs_per_mib_semantic = sample;
    } else {
        cur.ema_secs_per_mib_semantic =
            alpha * sample + (1.0 - alpha) * cur.ema_secs_per_mib_semantic;
    }
    cur.sample_count = cur.sample_count.saturating_add(1);
    let path = stats_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(&cur) {
        let _ = fs::write(path, json);
    }
}

pub fn format_duration_humans(secs: f64) -> String {
    if secs < 90.0 {
        format!("~{:.0}s", secs)
    } else if secs < 3600.0 {
        format!("~{:.1}m", secs / 60.0)
    } else {
        format!("~{:.1}h", secs / 3600.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_sanity() {
        assert!(format_duration_humans(30.0).contains('s'));
        assert!(format_duration_humans(120.0).contains('m'));
    }
}
