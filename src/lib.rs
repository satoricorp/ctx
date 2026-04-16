pub mod api;
pub mod artifact;
pub mod auth;
pub mod cli;
pub mod extraction;
pub mod index;
pub mod install;
pub mod mcp;
pub mod models;
pub mod retrieval;
pub mod store;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use extraction::classifier::{classify_content, ContentLayer};
use retrieval::query::{QueryResult, QueryType};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use artifact::{
    aura_path, blobs_path, context_path, context_root, index_path, manifest_path, Manifest,
    ManifestEntry,
};
use index::procedural::{ingest_procedural_document, record_procedure_structured};
use index::semantic::ingest_semantic_document;
use install::{ensure_base_dirs, load_config, save_config};
use store::get_or_open_env;
use store::schema::{AddOutcome, ContextListing, ContextStatus, RecordProcedureInput};

const BOOTSTRAP_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", "CONTEXT.md"];

pub async fn init_context(name: &str) -> Result<ContextStatus> {
    prepare_context_layout(name)?;
    bootstrap_procedural(name).await?;
    context_status(name)
}

/// Creates the context directory, manifest, and Helix env. Shared by **`init`** and first-**`add`**.
fn prepare_context_layout(name: &str) -> Result<PathBuf> {
    ensure_base_dirs()?;
    seed_default_config()?;

    let ctx_path = context_path(name);
    fs::create_dir_all(blobs_path(&ctx_path))?;
    fs::create_dir_all(index_path(&ctx_path))?;
    seed_aura_files(&ctx_path)?;

    let mut manifest = if manifest_path(&ctx_path).exists() {
        Manifest::load(&ctx_path)?
    } else {
        Manifest::empty(name)
    };
    manifest.name = name.to_string();
    sync_manifest_config(&mut manifest)?;
    refresh_aura_registry(&ctx_path, &mut manifest)?;
    manifest.save(&ctx_path)?;

    get_or_open_env(&index_path(&ctx_path))?;
    Ok(ctx_path)
}

const AURA_INDEX_SEED: &str = "# Aura Index\n\nAgent-maintained hub for the aura directory. \
Topic files appear below with one-line summaries.\n\n<!-- topics will be added here -->\n";

const AURA_MAIN_SEED: &str = "# Aura\n\nLong-term promoted memory for this context. \
Entries here are distilled from topic files via the promotion cycle.\n\n\
<!-- entries will appear here -->\n";

/// Creates the aura directory and seeds index.md and aura.md only if they don't already exist.
fn seed_aura_files(ctx_path: &Path) -> Result<()> {
    let dir = aura_path(ctx_path);
    fs::create_dir_all(&dir)?;
    let index = dir.join("index.md");
    if !index.exists() {
        fs::write(&index, AURA_INDEX_SEED)?;
    }
    let aura = dir.join("aura.md");
    if !aura.exists() {
        fs::write(&aura, AURA_MAIN_SEED)?;
    }
    Ok(())
}

pub async fn add_to_context(
    context: &str,
    path: &Path,
    layer: Option<ContentLayer>,
    with_content: bool,
) -> Result<AddOutcome> {
    let ctx_path = ensure_context_for_add(context)?;
    let abs = path
        .canonicalize()
        .with_context(|| format!("resolve {}", path.display()))?;

    if abs.is_dir() {
        let root = abs.clone();
        let mut total = AddOutcome::default();
        for entry in WalkDir::new(&root).into_iter().filter_map(|entry| entry.ok()) {
            let entry_path = entry.path();
            if !entry.file_type().is_file() || should_skip_path(entry_path) {
                continue;
            }
            let rel = entry_path
                .strip_prefix(&root)
                .unwrap_or(entry_path)
                .to_path_buf();
            let content = fs::read_to_string(entry_path).with_context(|| {
                format!("failed to read {} as utf-8 text", entry_path.display())
            })?;
            let outcome =
                add_file_buffer(&ctx_path, &root, &rel, &content, layer, with_content).await?;
            total.chunks_written += outcome.chunks_written;
            total.entities_written += outcome.entities_written;
        }
        return Ok(total);
    }

    let root = abs
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"));
    let rel = PathBuf::from(abs.file_name().context("file without a name")?);
    let content = fs::read_to_string(&abs)
        .with_context(|| format!("failed to read {} as utf-8 text", abs.display()))?;
    add_file_buffer(&ctx_path, &root, &rel, &content, layer, with_content).await
}

pub async fn add_content_to_context(
    context: &str,
    content: &str,
    source: Option<&str>,
    layer: Option<ContentLayer>,
) -> Result<AddOutcome> {
    let ctx_path = ensure_context_for_add(context)?;
    let source_id = source
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("inline/{}.txt", uuid::Uuid::new_v4()));
    add_inline_buffer(&ctx_path, &source_id, content, layer).await
}

pub async fn query_context(
    context: &str,
    query: &str,
    kind: QueryType,
    k: usize,
) -> Result<Vec<QueryResult>> {
    let ctx_path = open_existing_context(context)?;
    retrieval::query::query(query, kind, &ctx_path, k).await
}

pub async fn update_context(context: &str) -> Result<ContextStatus> {
    let ctx_path = open_existing_context(context)?;
    let manifest = Manifest::load(&ctx_path)?;
    let with_content = manifest.config.store_raw_content;

    let targets: Vec<(PathBuf, PathBuf, Option<ContentLayer>)> = manifest
        .sources
        .iter()
        .flat_map(|source| {
            let root = PathBuf::from(&source.root);
            source.files.iter().map(move |entry| {
                let rel = PathBuf::from(&entry.path);
                let layer = parse_layer(&entry.r#type).ok();
                (root.clone(), rel, layer)
            })
        })
        .collect();

    for (root, rel, layer) in targets {
        let abs = root.join(&rel);
        if !abs.exists() {
            continue;
        }
        let content = fs::read_to_string(&abs)
            .with_context(|| format!("failed to read {} as utf-8 text", abs.display()))?;
        add_file_buffer(&ctx_path, &root, &rel, &content, layer, with_content).await?;
    }

    let mut manifest = Manifest::load(&ctx_path)?;
    refresh_aura_registry(&ctx_path, &mut manifest)?;
    manifest.save(&ctx_path)?;

    context_status(context)
}

pub async fn record_procedure(context: &str, record: RecordProcedureInput) -> Result<String> {
    let ctx_path = open_existing_context(context)?;
    Ok(record_procedure_structured(&ctx_path, None, record, None)
        .await?
        .id)
}

pub fn list_contexts() -> Result<Vec<ContextListing>> {
    let root = context_root();
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut contexts = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() || path.extension().and_then(|ext| ext.to_str()) != Some("ctx") {
            continue;
        }

        let manifest = Manifest::load(&path)?;
        let env = get_or_open_env(&index_path(&path))?;
        let state = env.state();
        let updated_at = manifest
            .sources
            .iter()
            .flat_map(|source| source.files.iter())
            .map(|entry| entry.indexed_at)
            .max()
            .or_else(|| state.latest_update());

        contexts.push(ContextListing {
            name: manifest.name,
            counts: state.counts(),
            updated_at,
        });
    }

    contexts.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(contexts)
}

pub fn context_status(context: &str) -> Result<ContextStatus> {
    let ctx_path = open_existing_context(context)?;
    let mut manifest = Manifest::load(&ctx_path)?;
    let state = get_or_open_env(&index_path(&ctx_path))?.state();

    let mut indexed_count = 0usize;
    let mut dirty_count = 0usize;
    let mut pending_count = 0usize;
    let mut drifted_files: Vec<String> = Vec::new();
    let mut manifest_dirty = false;

    for source in manifest.sources.iter_mut() {
        let root = PathBuf::from(&source.root);
        for entry in source.files.iter_mut() {
            let abs = root.join(&entry.path);
            if !abs.exists() {
                pending_count += 1;
                continue;
            }
            let current_hash = hash_file(&abs)?;
            if current_hash != entry.hash {
                entry.hash = current_hash.clone();
                manifest_dirty = true;
            }
            if entry.hash != entry.hash_at_index {
                dirty_count += 1;
                drifted_files.push(abs.display().to_string());
            } else {
                indexed_count += 1;
            }
        }
    }

    drifted_files.sort();

    if manifest_dirty {
        manifest.save(&ctx_path)?;
    }

    Ok(ContextStatus {
        name: manifest.name,
        indexed_count,
        dirty_count,
        pending_count,
        counts: state.counts(),
        extraction_model: manifest.config.extraction_model,
        embedding_model: manifest.config.embedding_model,
        splade_enabled: manifest.config.splade_enabled,
        drifted_files,
    })
}

async fn bootstrap_procedural(context: &str) -> Result<()> {
    let cwd = std::env::current_dir()?;
    for file_name in BOOTSTRAP_FILES {
        let path = cwd.join(file_name);
        if path.exists() {
            add_to_context(context, &path, Some(ContentLayer::Procedural), false).await?;
        }
    }
    Ok(())
}

/// Ingests a filesystem-backed file and records it in the manifest under `root`/`rel_path`.
async fn add_file_buffer(
    ctx_path: &Path,
    root: &Path,
    rel_path: &Path,
    content: &str,
    layer: Option<ContentLayer>,
    with_content: bool,
) -> Result<AddOutcome> {
    let source_hash = hash_bytes(content.as_bytes());
    let root_str = root.display().to_string();
    let rel_str = rel_path.display().to_string();
    let source_id = root.join(rel_path).display().to_string();

    let mut manifest = Manifest::load(ctx_path)?;
    sync_manifest_config(&mut manifest)?;

    let raw_enabled = manifest.config.store_raw_content || with_content;

    if let Some(entry) = manifest.entry_for_mut(&root_str, &rel_str) {
        if entry.hash_at_index == source_hash && (!raw_enabled || entry.blob_ref.is_some()) {
            return Ok(AddOutcome::default());
        }
    }

    let hash_match = manifest
        .entry_for_mut(&root_str, &rel_str)
        .map(|entry| entry.hash_at_index == source_hash)
        .unwrap_or(false);

    let (chunk_count, entity_count) = if hash_match {
        (0, 0)
    } else {
        ingest_into_store(ctx_path, &source_id, content, layer, rel_path).await?
    };

    let mut entry_blob_ref: Option<String> = None;
    if raw_enabled {
        let written = artifact::blob::write_blob(ctx_path, content.as_bytes())?;
        entry_blob_ref = Some(written.blob_hash);
        if !manifest.config.store_raw_content {
            manifest.config.store_raw_content = true;
        }
    }

    let chosen_layer = layer.unwrap_or_else(|| classify_content(rel_path, content));
    let indexed_at = Utc::now();
    let source = manifest.upsert_source(&root_str);
    if let Some(entry) = source.files.iter_mut().find(|e| e.path == rel_str) {
        entry.hash = source_hash.clone();
        entry.hash_at_index = source_hash;
        entry.indexed_at = indexed_at;
        entry.r#type = chosen_layer.to_string();
        entry.blob_ref = entry_blob_ref;
    } else {
        source.files.push(ManifestEntry {
            path: rel_str,
            hash: source_hash.clone(),
            hash_at_index: source_hash,
            indexed_at,
            r#type: chosen_layer.to_string(),
            blob_ref: entry_blob_ref,
        });
    }
    manifest.save(ctx_path)?;

    Ok(AddOutcome {
        chunks_written: chunk_count,
        entities_written: entity_count,
    })
}

/// Ingests raw content (typically from the `ctx_add` MCP tool) without writing a manifest entry.
async fn add_inline_buffer(
    ctx_path: &Path,
    source_id: &str,
    content: &str,
    layer: Option<ContentLayer>,
) -> Result<AddOutcome> {
    let (chunk_count, entity_count) =
        ingest_into_store(ctx_path, source_id, content, layer, Path::new(source_id)).await?;
    Ok(AddOutcome {
        chunks_written: chunk_count,
        entities_written: entity_count,
    })
}

/// Removes any previous records for `source_id` and re-runs the chosen extractor.
async fn ingest_into_store(
    ctx_path: &Path,
    source_id: &str,
    content: &str,
    layer: Option<ContentLayer>,
    classifier_hint: &Path,
) -> Result<(usize, usize)> {
    let env = get_or_open_env(&index_path(ctx_path))?;
    env.update_state(|state| {
        state.remove_source(source_id);
        Ok(())
    })?;

    let chosen_layer = layer.unwrap_or_else(|| classify_content(classifier_hint, content));
    Ok(match chosen_layer {
        ContentLayer::Semantic => {
            let indexed = ingest_semantic_document(ctx_path, source_id, content).await?;
            (indexed.chunk_count, indexed.entity_count)
        }
        ContentLayer::Procedural => {
            ingest_procedural_document(ctx_path, Some(source_id.to_string()), content).await?;
            (0, 0)
        }
    })
}

fn seed_default_config() -> Result<()> {
    let path = install::config_path()?;
    if path.exists() {
        return Ok(());
    }

    save_config(&load_config().unwrap_or_default())
}

fn sync_manifest_config(manifest: &mut Manifest) -> Result<()> {
    let config = load_config().unwrap_or_default();
    manifest.config.splade_enabled = config.splade_enabled;
    manifest.config.extraction_model = config.extraction_model;
    manifest.config.embedding_model = config.embedding_model;
    Ok(())
}

pub fn open_existing_context(context: &str) -> Result<PathBuf> {
    let ctx_path = context_path(context);
    if !ctx_path.exists() {
        bail!("context {} does not exist", context);
    }
    Ok(ctx_path)
}

/// Ensures the context directory exists, initializing it when invoked from **`add`** paths.
fn ensure_context_for_add(context: &str) -> Result<PathBuf> {
    let ctx_path = context_path(context);
    if ctx_path.exists() {
        return Ok(ctx_path);
    }
    prepare_context_layout(context)?;
    eprintln!("created context {}", context);
    Ok(ctx_path)
}

fn should_skip_path(path: &Path) -> bool {
    use std::ffi::OsStr;

    if path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(".git" | ".ctx" | "target" | "node_modules")
        )
    }) {
        return true;
    }

    // macOS / Windows junk files (binary or non-UTF-8)
    match path.file_name() {
        Some(name) if name == OsStr::new(".DS_Store") || name == OsStr::new("Thumbs.db") => true,
        _ => false,
    }
}

fn parse_layer(value: &str) -> Result<ContentLayer> {
    match value {
        "semantic" => Ok(ContentLayer::Semantic),
        "procedural" => Ok(ContentLayer::Procedural),
        _ => bail!("unknown layer {}", value),
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn hash_file(path: &Path) -> Result<String> {
    Ok(hash_bytes(&fs::read(path)?))
}

// -------------------------------- drift --------------------------------

/// A source file whose recorded `hash` no longer matches `hash_at_index`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DriftedFile {
    pub root: String,
    pub path: String,
    pub hash: String,
    pub hash_at_index: String,
}

/// Aggregated drift state derived entirely from the manifest (no file I/O).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DriftState {
    pub drift_detected: bool,
    pub drifted_files: Vec<DriftedFile>,
}

/// Hint surfaced to callers when drift is detected.
pub const DRIFT_HINT: &str =
    "context may be stale; run `ctx update` to re-index drifted files";

/// Manifest-only drift check. Cheap: no file I/O against source roots.
pub fn drift_state(ctx_path: &Path) -> Result<DriftState> {
    let manifest = Manifest::load(ctx_path)?;
    let mut drifted_files: Vec<DriftedFile> = Vec::new();
    for source in &manifest.sources {
        for entry in &source.files {
            if entry.hash != entry.hash_at_index {
                drifted_files.push(DriftedFile {
                    root: source.root.clone(),
                    path: entry.path.clone(),
                    hash: entry.hash.clone(),
                    hash_at_index: entry.hash_at_index.clone(),
                });
            }
        }
    }
    drifted_files.sort_by(|a, b| (a.root.as_str(), a.path.as_str()).cmp(&(b.root.as_str(), b.path.as_str())));
    Ok(DriftState {
        drift_detected: !drifted_files.is_empty(),
        drifted_files,
    })
}

// -------------------------------- aura --------------------------------

/// Read-only snapshot of aura content for inclusion in query responses.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuraSummary {
    pub index: Option<String>,
    pub aura: Option<String>,
    pub topics: Vec<String>,
}

/// A single aura file read off disk, paired with its current hash.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuraContent {
    pub path: String,
    pub content: String,
    pub hash: String,
}

/// Write modes for `write_aura_file`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuraWriteMode {
    Replace,
    Append,
}

impl AuraWriteMode {
    pub fn parse(value: Option<&str>) -> Result<Self> {
        match value.unwrap_or("replace") {
            "replace" => Ok(Self::Replace),
            "append" => Ok(Self::Append),
            other => bail!("unknown aura write mode {}", other),
        }
    }
}

/// Outcome of a successful aura writeback.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuraWriteOutcome {
    pub path: String,
    pub hash: String,
}

/// Rescans `aura/` and rebuilds the manifest's aura registry. `updated_at` is preserved
/// for entries whose hash did not change; new or changed entries are stamped with `now`.
pub fn refresh_aura_registry(ctx_path: &Path, manifest: &mut Manifest) -> Result<()> {
    let dir = aura_path(ctx_path);
    if !dir.exists() {
        manifest.aura.files.clear();
        return Ok(());
    }

    let existing: std::collections::HashMap<String, artifact::manifest::AuraFile> = manifest
        .aura
        .files
        .drain(..)
        .map(|entry| (entry.path.clone(), entry))
        .collect();

    let mut refreshed: Vec<artifact::manifest::AuraFile> = Vec::new();
    for entry in WalkDir::new(&dir).min_depth(1).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let abs = entry.path();
        if abs.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let rel = abs.strip_prefix(ctx_path).unwrap_or(abs);
        let rel_str = rel.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
        let bytes = fs::read(abs)
            .with_context(|| format!("read aura file {}", abs.display()))?;
        let hash = hash_bytes(&bytes);
        let updated_at = match existing.get(&rel_str) {
            Some(prev) if prev.hash == hash => prev.updated_at,
            _ => Utc::now(),
        };
        refreshed.push(artifact::manifest::AuraFile {
            path: rel_str,
            hash,
            updated_at,
        });
    }

    refreshed.sort_by(|a, b| a.path.cmp(&b.path));
    manifest.aura.files = refreshed;
    Ok(())
}

/// Reads index.md (first) and aura.md (second), plus the list of other aura/*.md paths.
pub fn read_aura_summary(ctx_path: &Path) -> Result<AuraSummary> {
    let dir = aura_path(ctx_path);
    let read_opt = |name: &str| -> Result<Option<String>> {
        let path = dir.join(name);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(
            fs::read_to_string(&path)
                .with_context(|| format!("read {}", path.display()))?,
        ))
    };
    let index = read_opt("index.md")?;
    let aura = read_opt("aura.md")?;

    let mut topics: Vec<String> = Vec::new();
    if dir.exists() {
        for entry in WalkDir::new(&dir).min_depth(1).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }
            let abs = entry.path();
            if abs.extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }
            let name = abs.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "index.md" || name == "aura.md" {
                continue;
            }
            let rel = abs.strip_prefix(ctx_path).unwrap_or(abs);
            topics.push(rel.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"));
        }
    }
    topics.sort();

    Ok(AuraSummary { index, aura, topics })
}

/// Reads a single aura file. `rel_path` must start with `aura/` and must resolve inside
/// the artifact's aura directory.
pub fn read_aura_file(ctx_path: &Path, rel_path: &str) -> Result<AuraContent> {
    let abs = resolve_aura_path(ctx_path, rel_path)?;
    if !abs.exists() {
        bail!("aura file {} does not exist", rel_path);
    }
    let bytes = fs::read(&abs)
        .with_context(|| format!("read {}", abs.display()))?;
    let hash = hash_bytes(&bytes);
    let content = String::from_utf8(bytes)
        .with_context(|| format!("aura file {} is not valid utf-8", rel_path))?;
    Ok(AuraContent {
        path: normalize_aura_path(rel_path),
        content,
        hash,
    })
}

/// Writes (or appends to) an aura file and updates the manifest registry entry.
pub fn write_aura_file(
    context: &str,
    rel_path: &str,
    content: &str,
    mode: AuraWriteMode,
) -> Result<AuraWriteOutcome> {
    let ctx_path = open_existing_context(context)?;
    let abs = resolve_aura_path(&ctx_path, rel_path)?;

    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent)?;
    }

    match mode {
        AuraWriteMode::Replace => {
            fs::write(&abs, content).with_context(|| format!("write {}", abs.display()))?;
        }
        AuraWriteMode::Append => {
            let mut buffer = if abs.exists() {
                fs::read(&abs)
                    .with_context(|| format!("read {}", abs.display()))?
            } else {
                Vec::new()
            };
            if !buffer.is_empty() && !buffer.ends_with(b"\n") {
                buffer.push(b'\n');
            }
            buffer.extend_from_slice(content.as_bytes());
            fs::write(&abs, &buffer).with_context(|| format!("write {}", abs.display()))?;
        }
    }

    let bytes = fs::read(&abs)?;
    let hash = hash_bytes(&bytes);
    let normalized = normalize_aura_path(rel_path);

    let mut manifest = Manifest::load(&ctx_path)?;
    manifest.upsert_aura(&normalized, &hash);
    manifest.save(&ctx_path)?;

    Ok(AuraWriteOutcome {
        path: normalized,
        hash,
    })
}

/// Resolves `rel_path` to an absolute path inside `ctx_path/aura/`, rejecting any attempt
/// to escape the aura directory.
fn resolve_aura_path(ctx_path: &Path, rel_path: &str) -> Result<PathBuf> {
    let normalized = normalize_aura_path(rel_path);
    if !normalized.starts_with("aura/") {
        bail!("aura path must begin with \"aura/\"");
    }
    if normalized.split('/').any(|segment| segment == "..") {
        bail!("aura path must not contain \"..\" segments");
    }
    let abs = ctx_path.join(&normalized);
    let aura_root = aura_path(ctx_path);
    let check_base = aura_root.canonicalize().unwrap_or(aura_root.clone());
    let check_target = abs
        .parent()
        .map(|parent| parent.canonicalize().unwrap_or_else(|_| parent.to_path_buf()))
        .unwrap_or_else(|| abs.clone());
    if !check_target.starts_with(&check_base) {
        bail!("aura path {} escapes the aura directory", rel_path);
    }
    Ok(abs)
}

fn normalize_aura_path(rel_path: &str) -> String {
    rel_path.replace('\\', "/")
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, OnceLock};

    pub(crate) fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn inline_semantic_round_trip() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let tempdir = TempDir::new().expect("tempdir");
        let home_root = TempDir::new().expect("home root");

        let saved_openai = std::env::var("OPENAI_API_KEY").ok();
        let saved_anthropic = std::env::var("ANTHROPIC_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("ANTHROPIC_API_KEY");

        std::env::set_var("HOME", home_root.path());
        std::env::set_var("CTX_PATH", tempdir.path());
        std::env::set_var("CTX_DISABLE_FASTEMBED", "1");

        init_context("test-inline").await.expect("init context");
        add_content_to_context(
            "test-inline",
            "AuthService uses RS256 tokens and writes audit events.",
            Some("inline/auth.md"),
            Some(ContentLayer::Semantic),
        )
        .await
        .expect("add content");

        let results = query_context("test-inline", "what uses RS256", QueryType::Semantic, 3)
            .await
            .expect("query context");

        assert!(!results.is_empty());
        assert!(results
            .iter()
            .any(|result| result.summary.contains("AuthService")));

        std::env::remove_var("CTX_DISABLE_FASTEMBED");
        std::env::remove_var("CTX_PATH");
        std::env::remove_var("HOME");
        restore_optional_env("OPENAI_API_KEY", saved_openai.as_deref());
        restore_optional_env("ANTHROPIC_API_KEY", saved_anthropic.as_deref());
    }

    #[tokio::test]
    async fn inline_procedural_round_trip() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let tempdir = TempDir::new().expect("tempdir");
        let home_root = TempDir::new().expect("home root");

        let saved_openai = std::env::var("OPENAI_API_KEY").ok();
        let saved_anthropic = std::env::var("ANTHROPIC_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("ANTHROPIC_API_KEY");

        std::env::set_var("HOME", home_root.path());
        std::env::set_var("CTX_PATH", tempdir.path());
        std::env::set_var("CTX_DISABLE_FASTEMBED", "1");

        init_context("test-procedural").await.expect("init context");
        add_content_to_context(
            "test-procedural",
            "1. run cargo test\n2. deploy to staging\n3. verify health checks\nresult: success",
            Some("inline/runbook.md"),
            Some(ContentLayer::Procedural),
        )
        .await
        .expect("add content");

        let results = query_context(
            "test-procedural",
            "how do we deploy to staging",
            QueryType::Procedural,
            3,
        )
        .await
        .expect("query context");

        assert!(!results.is_empty());
        assert!(results.iter().any(|result| {
            result.content.contains("deploy")
                || result.summary.to_lowercase().contains("staging")
        }));

        std::env::remove_var("CTX_DISABLE_FASTEMBED");
        std::env::remove_var("CTX_PATH");
        std::env::remove_var("HOME");
        restore_optional_env("OPENAI_API_KEY", saved_openai.as_deref());
        restore_optional_env("ANTHROPIC_API_KEY", saved_anthropic.as_deref());
    }

    fn restore_optional_env(key: &str, value: Option<&str>) {
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    struct AuraTestEnv {
        _tempdir: TempDir,
        _home_root: TempDir,
        saved_openai: Option<String>,
        saved_anthropic: Option<String>,
    }

    impl Drop for AuraTestEnv {
        fn drop(&mut self) {
            std::env::remove_var("CTX_DISABLE_FASTEMBED");
            std::env::remove_var("CTX_PATH");
            std::env::remove_var("HOME");
            restore_optional_env("OPENAI_API_KEY", self.saved_openai.as_deref());
            restore_optional_env("ANTHROPIC_API_KEY", self.saved_anthropic.as_deref());
        }
    }

    fn setup_aura_env() -> AuraTestEnv {
        let tempdir = TempDir::new().expect("tempdir");
        let home_root = TempDir::new().expect("home root");

        let saved_openai = std::env::var("OPENAI_API_KEY").ok();
        let saved_anthropic = std::env::var("ANTHROPIC_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("ANTHROPIC_API_KEY");

        std::env::set_var("HOME", home_root.path());
        std::env::set_var("CTX_PATH", tempdir.path());
        std::env::set_var("CTX_DISABLE_FASTEMBED", "1");

        AuraTestEnv {
            _tempdir: tempdir,
            _home_root: home_root,
            saved_openai,
            saved_anthropic,
        }
    }

    #[tokio::test]
    async fn aura_seeded_on_init() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_aura_env();

        init_context("aura-seed").await.expect("init context");
        let ctx_path = open_existing_context("aura-seed").expect("open context");

        let aura_dir = aura_path(&ctx_path);
        assert!(aura_dir.join("index.md").exists());
        assert!(aura_dir.join("aura.md").exists());

        let manifest = Manifest::load(&ctx_path).expect("load manifest");
        let paths: Vec<&str> = manifest
            .aura
            .files
            .iter()
            .map(|entry| entry.path.as_str())
            .collect();
        assert!(paths.contains(&"aura/index.md"));
        assert!(paths.contains(&"aura/aura.md"));
        for entry in &manifest.aura.files {
            assert!(!entry.hash.is_empty());
        }
    }

    #[tokio::test]
    async fn aura_writeback_round_trip() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_aura_env();

        init_context("aura-write").await.expect("init context");

        let outcome = write_aura_file(
            "aura-write",
            "aura/auth.md",
            "# Auth notes\n\nRS256 tokens only.\n",
            AuraWriteMode::Replace,
        )
        .expect("write aura file");
        assert_eq!(outcome.path, "aura/auth.md");
        assert!(!outcome.hash.is_empty());

        let ctx_path = open_existing_context("aura-write").expect("open context");
        let content = read_aura_file(&ctx_path, "aura/auth.md").expect("read aura file");
        assert!(content.content.contains("RS256 tokens only"));
        assert_eq!(content.hash, outcome.hash);

        let manifest = Manifest::load(&ctx_path).expect("load manifest");
        let entry = manifest
            .aura
            .files
            .iter()
            .find(|entry| entry.path == "aura/auth.md")
            .expect("manifest entry");
        assert_eq!(entry.hash, outcome.hash);

        write_aura_file(
            "aura-write",
            "aura/auth.md",
            "Additional note.\n",
            AuraWriteMode::Append,
        )
        .expect("append aura file");
        let appended = read_aura_file(&ctx_path, "aura/auth.md").expect("re-read");
        assert!(appended.content.contains("RS256 tokens only"));
        assert!(appended.content.contains("Additional note."));
    }

    #[tokio::test]
    async fn aura_read_tool_round_trip() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_aura_env();

        init_context("aura-read").await.expect("init context");
        write_aura_file(
            "aura-read",
            "aura/deploy.md",
            "deploy steps",
            AuraWriteMode::Replace,
        )
        .expect("seed topic file");

        let ctx_path = open_existing_context("aura-read").expect("open context");
        let summary = read_aura_summary(&ctx_path).expect("read summary");
        assert!(summary.index.is_some());
        assert!(summary.aura.is_some());
        assert!(summary.topics.contains(&String::from("aura/deploy.md")));

        let content = read_aura_file(&ctx_path, "aura/deploy.md").expect("read file");
        assert_eq!(content.path, "aura/deploy.md");
        assert_eq!(content.content, "deploy steps");
    }

    #[tokio::test]
    async fn drift_state_reads_manifest_only() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_aura_env();

        init_context("drift-manifest").await.expect("init context");
        let ctx_path = open_existing_context("drift-manifest").expect("open context");

        let src_dir = TempDir::new().expect("src dir");
        let file_path = src_dir.path().join("note.md");
        fs::write(&file_path, "original content").expect("write src");

        add_to_context(
            "drift-manifest",
            &file_path,
            Some(ContentLayer::Semantic),
            false,
        )
        .await
        .expect("add to context");

        let baseline = drift_state(&ctx_path).expect("drift baseline");
        assert!(!baseline.drift_detected);
        assert!(baseline.drifted_files.is_empty());

        let mut manifest = Manifest::load(&ctx_path).expect("load manifest");
        let entry = manifest
            .sources
            .first_mut()
            .and_then(|source| source.files.first_mut())
            .expect("entry");
        entry.hash = String::from("sha256:deadbeef");
        manifest.save(&ctx_path).expect("save manifest");

        fs::remove_file(&file_path).expect("remove source file");

        let drifted = drift_state(&ctx_path).expect("drift after mutation");
        assert!(drifted.drift_detected);
        assert_eq!(drifted.drifted_files.len(), 1);
        assert_eq!(drifted.drifted_files[0].hash, "sha256:deadbeef");
    }

    #[tokio::test]
    async fn status_rehash_updates_manifest_hash() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_aura_env();

        init_context("drift-status").await.expect("init context");
        let ctx_path = open_existing_context("drift-status").expect("open context");

        let src_dir = TempDir::new().expect("src dir");
        let file_path = src_dir.path().join("note.md");
        fs::write(&file_path, "original content").expect("write src");

        add_to_context("drift-status", &file_path, Some(ContentLayer::Semantic), false)
            .await
            .expect("add to context");

        let manifest_before = Manifest::load(&ctx_path).expect("load manifest");
        let entry_before = manifest_before
            .sources
            .first()
            .and_then(|source| source.files.first())
            .cloned()
            .expect("entry before");
        assert_eq!(entry_before.hash, entry_before.hash_at_index);

        fs::write(&file_path, "changed content").expect("mutate src");

        let status = context_status("drift-status").expect("status");
        assert_eq!(status.dirty_count, 1);
        assert_eq!(status.drifted_files.len(), 1);
        assert!(status.drifted_files[0].contains("note.md"));

        let manifest_after = Manifest::load(&ctx_path).expect("reload manifest");
        let entry_after = manifest_after
            .sources
            .first()
            .and_then(|source| source.files.first())
            .cloned()
            .expect("entry after");
        assert_ne!(entry_after.hash, entry_before.hash);
        assert_eq!(entry_after.hash_at_index, entry_before.hash_at_index);
    }

    #[tokio::test]
    async fn query_surfaces_drift_after_status() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_aura_env();

        init_context("drift-query").await.expect("init context");
        let ctx_path = open_existing_context("drift-query").expect("open context");

        let src_dir = TempDir::new().expect("src dir");
        let file_path = src_dir.path().join("note.md");
        fs::write(&file_path, "AuthService uses RS256 tokens.").expect("write src");

        add_to_context("drift-query", &file_path, Some(ContentLayer::Semantic), false)
            .await
            .expect("add to context");

        let before = drift_state(&ctx_path).expect("drift before");
        assert!(!before.drift_detected);

        fs::write(&file_path, "AuthService uses RS512 tokens now.").expect("mutate src");
        context_status("drift-query").expect("status to persist new hash");

        let after = drift_state(&ctx_path).expect("drift after");
        assert!(after.drift_detected);
    }

    #[tokio::test]
    async fn update_clears_drift() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_aura_env();

        init_context("drift-update").await.expect("init context");
        let ctx_path = open_existing_context("drift-update").expect("open context");

        let src_dir = TempDir::new().expect("src dir");
        let file_path = src_dir.path().join("note.md");
        fs::write(&file_path, "initial").expect("write src");

        add_to_context("drift-update", &file_path, Some(ContentLayer::Semantic), false)
            .await
            .expect("add to context");

        fs::write(&file_path, "updated body").expect("mutate src");
        let status = context_status("drift-update").expect("status");
        assert_eq!(status.dirty_count, 1);
        assert!(drift_state(&ctx_path).expect("drift set").drift_detected);

        update_context("drift-update").await.expect("update context");

        let post = drift_state(&ctx_path).expect("drift after update");
        assert!(!post.drift_detected);

        let manifest = Manifest::load(&ctx_path).expect("load manifest");
        let entry = manifest
            .sources
            .first()
            .and_then(|source| source.files.first())
            .expect("entry post-update");
        assert_eq!(entry.hash, entry.hash_at_index);
    }

    #[tokio::test]
    async fn aura_refresh_detects_external_edit() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_aura_env();

        init_context("aura-drift").await.expect("init context");
        let ctx_path = open_existing_context("aura-drift").expect("open context");

        let manifest_before = Manifest::load(&ctx_path).expect("load manifest");
        let before_hash = manifest_before
            .aura
            .files
            .iter()
            .find(|entry| entry.path == "aura/index.md")
            .expect("index entry")
            .hash
            .clone();

        let index_path = aura_path(&ctx_path).join("index.md");
        fs::write(&index_path, "# Aura Index\n\nEdited externally.\n")
            .expect("external edit");

        update_context("aura-drift").await.expect("update context");

        let manifest_after = Manifest::load(&ctx_path).expect("reload manifest");
        let after = manifest_after
            .aura
            .files
            .iter()
            .find(|entry| entry.path == "aura/index.md")
            .expect("index entry after refresh");
        assert_ne!(after.hash, before_hash);
    }

    fn count_blobs(ctx_path: &Path) -> usize {
        let dir = artifact::blobs_path(ctx_path);
        if !dir.exists() {
            return 0;
        }
        fs::read_dir(&dir)
            .expect("read blobs dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().map(|ft| ft.is_file()).unwrap_or(false))
            .count()
    }

    #[tokio::test]
    async fn add_with_content_writes_blob_and_flips_config() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_aura_env();

        init_context("blob-add").await.expect("init context");
        let ctx_path = open_existing_context("blob-add").expect("open context");

        let src_dir = TempDir::new().expect("src dir");
        let file_path = src_dir.path().join("note.md");
        fs::write(&file_path, "raw content body").expect("write src");

        add_to_context("blob-add", &file_path, Some(ContentLayer::Semantic), true)
            .await
            .expect("add with content");

        let manifest = Manifest::load(&ctx_path).expect("load manifest");
        assert!(manifest.config.store_raw_content);

        let entry = manifest
            .sources
            .first()
            .and_then(|source| source.files.first())
            .expect("entry");
        let blob_ref = entry.blob_ref.as_deref().expect("blob ref populated");
        assert!(blob_ref.starts_with("sha256:"));

        let blob_path = artifact::blobs_path(&ctx_path)
            .join(blob_ref.trim_start_matches("sha256:"));
        assert!(blob_path.exists(), "blob file {} missing", blob_path.display());
    }

    #[tokio::test]
    async fn add_without_with_content_leaves_blob_unset() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_aura_env();

        init_context("blob-default").await.expect("init context");
        let ctx_path = open_existing_context("blob-default").expect("open context");

        let src_dir = TempDir::new().expect("src dir");
        let file_path = src_dir.path().join("note.md");
        fs::write(&file_path, "hash only body").expect("write src");

        add_to_context("blob-default", &file_path, Some(ContentLayer::Semantic), false)
            .await
            .expect("add without content");

        let manifest = Manifest::load(&ctx_path).expect("load manifest");
        assert!(!manifest.config.store_raw_content);
        let entry = manifest
            .sources
            .first()
            .and_then(|source| source.files.first())
            .expect("entry");
        assert!(entry.blob_ref.is_none());
        assert_eq!(count_blobs(&ctx_path), 0);
    }

    #[tokio::test]
    async fn add_dedupe_writes_blob_once() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_aura_env();

        init_context("blob-dedupe").await.expect("init context");
        let ctx_path = open_existing_context("blob-dedupe").expect("open context");

        let src_dir = TempDir::new().expect("src dir");
        let body = "identical bytes across files";
        let a = src_dir.path().join("a.md");
        let b = src_dir.path().join("b.md");
        fs::write(&a, body).expect("write a");
        fs::write(&b, body).expect("write b");

        add_to_context("blob-dedupe", &a, Some(ContentLayer::Semantic), true)
            .await
            .expect("add a");
        add_to_context("blob-dedupe", &b, Some(ContentLayer::Semantic), true)
            .await
            .expect("add b");

        assert_eq!(count_blobs(&ctx_path), 1);
    }

    #[tokio::test]
    async fn update_writes_new_blob_on_drift() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_aura_env();

        init_context("blob-drift").await.expect("init context");
        let ctx_path = open_existing_context("blob-drift").expect("open context");

        let src_dir = TempDir::new().expect("src dir");
        let file_path = src_dir.path().join("note.md");
        fs::write(&file_path, "initial body").expect("write src");

        add_to_context("blob-drift", &file_path, Some(ContentLayer::Semantic), true)
            .await
            .expect("add with content");

        let manifest_before = Manifest::load(&ctx_path).expect("load manifest");
        let blob_before = manifest_before
            .sources
            .first()
            .and_then(|source| source.files.first())
            .and_then(|entry| entry.blob_ref.clone())
            .expect("blob_ref before");

        fs::write(&file_path, "mutated body").expect("mutate src");
        update_context("blob-drift").await.expect("update");

        let manifest_after = Manifest::load(&ctx_path).expect("reload manifest");
        let blob_after = manifest_after
            .sources
            .first()
            .and_then(|source| source.files.first())
            .and_then(|entry| entry.blob_ref.clone())
            .expect("blob_ref after");

        assert_ne!(blob_before, blob_after, "expected new blob_ref after drift");
        assert_eq!(
            count_blobs(&ctx_path),
            2,
            "old blob MUST remain (blobs are immutable, no GC)"
        );
    }
}
