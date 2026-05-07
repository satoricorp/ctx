pub mod api;
pub mod artifact;
pub mod auth;
pub mod cli;
pub mod doctor;
pub mod extraction;
pub mod index;
pub mod index_job;
pub mod index_plan;
pub mod install;
pub mod mcp;
pub mod models;
pub mod notes_update;
pub mod retrieval;
pub mod store;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use extraction::classifier::{classify_content, ContentLayer};
use extraction::decoder::{
    any_binary_decoder_claims, decode_file, DecodedUnit, PlainTextUnitStream, PEEK_SNIFF_BYTES,
    PLAIN_TEXT_STREAM_THRESHOLD,
};
use retrieval::query::{QueryResult, QueryType};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

use artifact::{
    blobs_path, context_path, context_root, index_path, manifest_path, notes_path, Manifest,
    ManifestEntry,
};
use index::procedural::{ingest_procedural_document, record_procedure_structured};
use index::semantic::ingest_semantic_document;
use install::{
    ensure_base_dirs, ensure_local_setup, load_config, save_config, set_default_context_if_unset,
};
use store::get_or_open_env;
use store::schema::{
    AddOutcome, ContextListing, ContextStatus, IngestionSummary, RecordProcedureInput,
};

const BOOTSTRAP_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", "CONTEXT.md"];

pub async fn init_context(name: &str) -> Result<ContextStatus> {
    ensure_local_setup()?;
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
    migrate_legacy_notes_dir(&ctx_path)?;
    seed_notes_files(&ctx_path)?;

    let mut manifest = if manifest_path(&ctx_path).exists() {
        Manifest::load(&ctx_path)?
    } else {
        Manifest::empty(name)
    };
    manifest.name = name.to_string();
    sync_manifest_config(&mut manifest)?;
    refresh_notes_registry(&ctx_path, &mut manifest)?;
    manifest.save(&ctx_path)?;

    get_or_open_env(&index_path(&ctx_path))?;
    set_default_context_if_unset(name)?;
    Ok(ctx_path)
}

const NOTES_INDEX_SEED: &str = "# Notes Index\n\nAgent-maintained hub for the notes directory. \
Topic files appear below with one-line summaries.\n\n<!-- topics will be added here -->\n";

const NOTES_SUMMARY_SEED: &str = "# Notes Summary\n\nLong-term project notes for this context. \
Entries here are distilled from stable topic notes during the update cycle.\n\n\
<!-- entries will appear here -->\n";

/// Creates the notes directory and seeds index.md and summary.md only if they don't already exist.
fn seed_notes_files(ctx_path: &Path) -> Result<()> {
    let dir = notes_path(ctx_path);
    fs::create_dir_all(&dir)?;
    let index = dir.join("index.md");
    if !index.exists() {
        fs::write(&index, NOTES_INDEX_SEED)?;
    }
    let summary = dir.join("summary.md");
    if !summary.exists() {
        fs::write(&summary, NOTES_SUMMARY_SEED)?;
    }
    Ok(())
}

fn migrate_legacy_notes_dir(ctx_path: &Path) -> Result<()> {
    let notes_dir = notes_path(ctx_path);
    if !notes_dir.exists() {
        let legacy_dir = ctx_path.join("aura");
        if legacy_dir.exists() {
            fs::rename(&legacy_dir, &notes_dir).with_context(|| {
                format!(
                    "rename legacy notes directory {} -> {}",
                    legacy_dir.display(),
                    notes_dir.display()
                )
            })?;
        }
    }

    let legacy_summary = notes_dir.join("aura.md");
    let summary = notes_dir.join("summary.md");
    if legacy_summary.exists() && !summary.exists() {
        fs::rename(&legacy_summary, &summary).with_context(|| {
            format!(
                "rename legacy notes summary {} -> {}",
                legacy_summary.display(),
                summary.display()
            )
        })?;
    }

    Ok(())
}

pub async fn add_to_context(
    context: &str,
    path: &Path,
    layer: Option<ContentLayer>,
    with_content: bool,
) -> Result<AddOutcome> {
    add_to_context_with_verbosity(context, path, layer, with_content, false).await
}

pub async fn add_to_context_with_verbosity(
    context: &str,
    path: &Path,
    layer: Option<ContentLayer>,
    with_content: bool,
    verbose: bool,
) -> Result<AddOutcome> {
    let ctx_path = ensure_context_for_add(context)?;
    let abs = path
        .canonicalize()
        .with_context(|| format!("resolve {}", path.display()))?;

    if abs.is_dir() {
        let plan = index_plan::plan_add_directory(&ctx_path, &abs, layer, with_content)?;
        let mut summary = run_ingest_work_items(&ctx_path, &plan.items, verbose).await?;
        summary.files_seen = plan.stats.files_seen;
        summary.files_skipped_denylist = plan.stats.skipped_denylist;
        summary.files_skipped_too_large += plan.stats.skipped_too_large;
        summary.files_skipped_read_error += plan.stats.skipped_stat_error;
        // Already-indexed files were excluded from `items`; count as neither decoded nor decode-error.
        eprintln!("{}", summary.format_oneline());
        return Ok(AddOutcome {
            chunks_written: summary.chunks_written,
            entities_written: summary.entities_written,
        });
    }

    let root = abs
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"));
    let rel = PathBuf::from(abs.file_name().context("file without a name")?);
    match ingest_source_from_path(&ctx_path, &root, &rel, &abs, layer, with_content).await {
        IngestOutcomeKind::Decoded { outcome, .. } => Ok(outcome),
        IngestOutcomeKind::TooLarge { size } => {
            bail!("file size {size} exceeds binary decoder cap")
        }
        IngestOutcomeKind::ReadError(err)
        | IngestOutcomeKind::DecodeError(err)
        | IngestOutcomeKind::EncodingError(err) => Err(err),
    }
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

/// Drops the on-disk index and re-ingests every manifest entry from its original source.
/// Blobs are preserved; only the index and the `hash_at_index` bookkeeping are reset.
pub async fn rebuild_index(context: &str) -> Result<ContextStatus> {
    let ctx_path = open_existing_context(context)?;
    let idx = index_path(&ctx_path);

    store::evict_env(&idx);
    if idx.exists() {
        fs::remove_dir_all(&idx).with_context(|| format!("remove {}", idx.display()))?;
    }

    let mut manifest = Manifest::load(&ctx_path)?;
    for source in manifest.sources.iter_mut() {
        for entry in source.files.iter_mut() {
            entry.hash_at_index.clear();
        }
    }
    manifest.save(&ctx_path)?;

    update_context(context).await
}

pub async fn update_context(context: &str) -> Result<ContextStatus> {
    update_context_with_verbosity(context, false).await
}

pub async fn update_context_with_verbosity(context: &str, verbose: bool) -> Result<ContextStatus> {
    let ctx_path = open_existing_context(context)?;
    let manifest = Manifest::load(&ctx_path)?;
    let with_content = manifest.config.store_raw_content;
    let plan = index_plan::plan_manifest_update(&ctx_path, with_content)?;
    let mut summary = run_ingest_work_items(&ctx_path, &plan.items, verbose).await?;
    summary.files_seen = plan.stats.files_seen;
    eprintln!("{}", summary.format_oneline());

    finalize_update_context(context, &ctx_path).await
}

/// Ingest only planned work items (shared by sync add/update and background jobs).
pub async fn run_ingest_work_items(
    ctx_path: &Path,
    items: &[index_plan::WorkItem],
    verbose: bool,
) -> Result<IngestionSummary> {
    let mut summary = IngestionSummary::default();
    for item in items {
        let abs = &item.abs;
        match ingest_source_from_path(
            ctx_path,
            &item.root,
            &item.rel,
            abs,
            item.layer,
            item.with_content,
        )
        .await
        {
            IngestOutcomeKind::Decoded {
                outcome,
                bytes_read,
            } => {
                summary.files_decoded += 1;
                summary.bytes_read += bytes_read;
                summary.units_written += 1;
                summary.chunks_written += outcome.chunks_written;
                summary.entities_written += outcome.entities_written;
            }
            IngestOutcomeKind::TooLarge { size } => {
                summary.files_skipped_too_large += 1;
                if verbose {
                    eprintln!(
                        "skipped {}: file size {} exceeds binary decoder cap",
                        abs.display(),
                        size
                    );
                }
            }
            IngestOutcomeKind::ReadError(err) => {
                summary.files_skipped_read_error += 1;
                if verbose {
                    eprintln!("skipped {}: {err:#}", abs.display());
                }
            }
            IngestOutcomeKind::DecodeError(err) => {
                summary.files_skipped_decode_error += 1;
                if verbose {
                    eprintln!("skipped {}: {err:#}", abs.display());
                }
            }
            IngestOutcomeKind::EncodingError(err) => {
                summary.files_skipped_encoding_error += 1;
                if verbose {
                    eprintln!("skipped {}: {err:#}", abs.display());
                }
            }
        }
    }
    Ok(summary)
}

/// Notes registry refresh + optional notes distill + status (end of `ctx update`).
pub async fn finalize_update_context(context: &str, ctx_path: &Path) -> Result<ContextStatus> {
    let mut manifest = Manifest::load(ctx_path)?;
    refresh_notes_registry(ctx_path, &mut manifest)?;
    manifest.save(ctx_path)?;

    if let Err(err) = notes_update::update_notes(ctx_path).await {
        eprintln!("notes update skipped: {err:#}");
    }

    context_status(context)
}

pub async fn record_procedure(context: &str, record: RecordProcedureInput) -> Result<String> {
    let ctx_path = open_existing_context(context)?;
    Ok(record_procedure_structured(&ctx_path, None, record, None)
        .await?
        .id)
}

/// Context names from `~/.ctx/contexts/*.ctx` directory stems, sorted.
///
/// Does not open index databases or read `manifest.json` — use for fast enumeration (e.g. `ctx list`).
pub fn list_context_names() -> Result<Vec<String>> {
    let root = context_root();
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut names = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() || path.extension().and_then(|ext| ext.to_str()) != Some("ctx") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        names.push(stem.to_string());
    }
    names.sort();
    Ok(names)
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
        let mut hashed: std::collections::HashMap<String, Option<String>> =
            std::collections::HashMap::new();
        for entry in source.files.iter_mut() {
            let source_path = entry.effective_source_path().to_string();
            let abs = root.join(&source_path);
            let current_hash = match hashed.get(&source_path) {
                Some(cached) => cached.clone(),
                None => {
                    let computed = if abs.exists() {
                        Some(hash_file(&abs)?)
                    } else {
                        None
                    };
                    hashed.insert(source_path.clone(), computed.clone());
                    computed
                }
            };
            let Some(current_hash) = current_hash else {
                pending_count += 1;
                continue;
            };
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
    drifted_files.dedup();

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

/// Returns `true` when every manifest entry for `(root, source_rel)` already has the matching
/// `source_hash` and required blob state, meaning there's nothing to ingest.
pub(crate) fn already_fully_indexed(
    ctx_path: &Path,
    root_str: &str,
    source_rel_str: &str,
    source_hash: &str,
    with_content: bool,
) -> Result<bool> {
    let manifest = Manifest::load(ctx_path)?;
    let raw_enabled = manifest.config.store_raw_content || with_content;
    if let Some(source) = manifest.sources.iter().find(|s| s.root == root_str) {
        let entries: Vec<&ManifestEntry> = source
            .files
            .iter()
            .filter(|entry| entry.effective_source_path() == source_rel_str)
            .collect();
        if !entries.is_empty()
            && entries.iter().all(|entry| {
                entry.hash_at_index == source_hash && (!raw_enabled || entry.blob_ref.is_some())
            })
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Ingest a single decoded unit, recording its relative path so the caller can later reconcile
/// orphan manifest entries from earlier decodes.
async fn ingest_one_unit(
    ctx_path: &Path,
    root: &Path,
    source_rel: &Path,
    unit: DecodedUnit,
    source_hash: &str,
    layer: Option<ContentLayer>,
    with_content: bool,
    source_stat: Option<SourceStat>,
    new_unit_paths: &mut Vec<String>,
    total: &mut AddOutcome,
) -> Result<()> {
    let unit_rel = if unit.virtual_path.as_os_str().is_empty() {
        source_rel.to_path_buf()
    } else {
        source_rel.join(&unit.virtual_path)
    };
    let source_path_for_entry = if unit.virtual_path.as_os_str().is_empty() {
        None
    } else {
        Some(source_rel)
    };
    new_unit_paths.push(unit_rel.display().to_string());

    let outcome = add_file_buffer(
        ctx_path,
        root,
        &unit_rel,
        &unit.text,
        layer,
        with_content,
        source_hash,
        source_path_for_entry,
        source_stat,
    )
    .await?;
    total.chunks_written += outcome.chunks_written;
    total.entities_written += outcome.entities_written;
    Ok(())
}

/// Remove manifest/index entries under `(root, source_rel)` whose paths no longer appear in the
/// latest decode's unit set. Runs at the end of every successful ingest.
fn prune_orphan_units(
    ctx_path: &Path,
    root: &Path,
    source_rel_str: &str,
    new_unit_paths: &[String],
) -> Result<()> {
    let root_str = root.display().to_string();
    let orphan_paths: Vec<String> = {
        let manifest = Manifest::load(ctx_path)?;
        manifest
            .sources
            .iter()
            .find(|s| s.root == root_str)
            .map(|source| {
                source
                    .files
                    .iter()
                    .filter(|entry| {
                        entry.effective_source_path() == source_rel_str
                            && !new_unit_paths.contains(&entry.path)
                    })
                    .map(|entry| entry.path.clone())
                    .collect()
            })
            .unwrap_or_default()
    };
    if orphan_paths.is_empty() {
        return Ok(());
    }
    let mut manifest = Manifest::load(ctx_path)?;
    if let Some(source) = manifest.source_for_mut(&root_str) {
        source
            .files
            .retain(|entry| !orphan_paths.contains(&entry.path));
    }
    manifest.save(ctx_path)?;
    let env = get_or_open_env(&index_path(ctx_path))?;
    env.update_state(|state| {
        for orphan in &orphan_paths {
            let orphan_source_id = root.join(orphan).display().to_string();
            state.remove_source(&orphan_source_id);
        }
        Ok(())
    })?;
    Ok(())
}

/// Top-level ingestion entry for a filesystem-backed source file. Hashes `bytes` once, decodes
/// into one or more units, ingests each, and removes any manifest/index records left over from
/// an earlier decode that no longer appear in the new unit set.
async fn add_source_file(
    ctx_path: &Path,
    root: &Path,
    source_rel: &Path,
    bytes: &[u8],
    layer: Option<ContentLayer>,
    with_content: bool,
) -> Result<AddOutcome> {
    let source_hash = hash_bytes(bytes);
    let root_str = root.display().to_string();
    let source_rel_str = source_rel.display().to_string();

    if already_fully_indexed(
        ctx_path,
        &root_str,
        &source_rel_str,
        &source_hash,
        with_content,
    )? {
        return Ok(AddOutcome::default());
    }

    let source_abs = root.join(source_rel);
    let units = decode_file(&source_abs, bytes)
        .with_context(|| format!("failed to decode {}", source_abs.display()))?;

    let mut total = AddOutcome::default();
    let mut new_unit_paths: Vec<String> = Vec::with_capacity(units.len());

    for unit in units {
        ingest_one_unit(
            ctx_path,
            root,
            source_rel,
            unit,
            &source_hash,
            layer,
            with_content,
            None,
            &mut new_unit_paths,
            &mut total,
        )
        .await?;
    }

    prune_orphan_units(ctx_path, root, &source_rel_str, &new_unit_paths)?;
    Ok(total)
}

/// Streaming ingestion for large plain-text sources. Hashes the file in a first streaming pass,
/// fast-paths if unchanged, then stream-decodes plain text into multiple `chunk-NNNN.txt`
/// virtual units without ever holding the full file in memory.
async fn add_source_file_streamed(
    ctx_path: &Path,
    root: &Path,
    source_rel: &Path,
    source_abs: &Path,
    source_stat: SourceStat,
    layer: Option<ContentLayer>,
    with_content: bool,
) -> Result<AddOutcome> {
    let root_str = root.display().to_string();
    let source_rel_str = source_rel.display().to_string();

    if already_fully_indexed_by_stat(
        ctx_path,
        &root_str,
        &source_rel_str,
        source_stat,
        with_content,
    )? {
        return Ok(AddOutcome::default());
    }

    let source_hash = hash_file_streaming(source_abs)?;

    if already_fully_indexed(
        ctx_path,
        &root_str,
        &source_rel_str,
        &source_hash,
        with_content,
    )? {
        return Ok(AddOutcome::default());
    }

    let file = File::open(source_abs).with_context(|| format!("open {}", source_abs.display()))?;
    let reader = BufReader::new(file);
    let mut stream = PlainTextUnitStream::new(reader);

    let mut total = AddOutcome::default();
    let mut new_unit_paths: Vec<String> = Vec::new();
    while let Some(unit) = stream
        .next_unit()
        .with_context(|| format!("stream-decode {}", source_abs.display()))?
    {
        ingest_one_unit(
            ctx_path,
            root,
            source_rel,
            unit,
            &source_hash,
            layer,
            with_content,
            Some(source_stat),
            &mut new_unit_paths,
            &mut total,
        )
        .await?;
    }

    prune_orphan_units(ctx_path, root, &source_rel_str, &new_unit_paths)?;
    Ok(total)
}

/// Ingests a single decoded unit and records it in the manifest under `root`/`rel_path`.
/// `source_hash` is the digest of the original source-file bytes (shared across all virtual
/// units from the same file); `source_path` is the real file path when `rel_path` is virtual.
async fn add_file_buffer(
    ctx_path: &Path,
    root: &Path,
    rel_path: &Path,
    content: &str,
    layer: Option<ContentLayer>,
    with_content: bool,
    source_hash: &str,
    source_path: Option<&Path>,
    source_stat: Option<SourceStat>,
) -> Result<AddOutcome> {
    let root_str = root.display().to_string();
    let rel_str = rel_path.display().to_string();
    let source_id = root.join(rel_path).display().to_string();
    let source_path_str = source_path.map(|p| p.display().to_string());

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
        entry.hash = source_hash.to_string();
        entry.hash_at_index = source_hash.to_string();
        entry.indexed_at = indexed_at;
        entry.r#type = chosen_layer.to_string();
        entry.blob_ref = entry_blob_ref;
        entry.source_path = source_path_str;
        set_entry_source_stat(entry, source_stat);
    } else {
        let mut entry = ManifestEntry {
            path: rel_str,
            hash: source_hash.to_string(),
            hash_at_index: source_hash.to_string(),
            indexed_at,
            r#type: chosen_layer.to_string(),
            blob_ref: entry_blob_ref,
            source_path: source_path_str,
            extra: Default::default(),
        };
        set_entry_source_stat(&mut entry, source_stat);
        source.files.push(entry);
    }
    manifest.save(ctx_path)?;

    Ok(AddOutcome {
        chunks_written: chunk_count,
        entities_written: entity_count,
    })
}

fn set_entry_source_stat(entry: &mut ManifestEntry, source_stat: Option<SourceStat>) {
    match source_stat {
        Some(stat) => {
            entry
                .extra
                .insert("source_size".into(), Value::from(stat.size));
            entry
                .extra
                .insert("source_mtime_unix".into(), Value::from(stat.mtime_unix));
        }
        None => {
            entry.extra.remove("source_size");
            entry.extra.remove("source_mtime_unix");
        }
    }
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
pub fn ensure_context_for_add(context: &str) -> Result<PathBuf> {
    let ctx_path = context_path(context);
    if ctx_path.exists() {
        return Ok(ctx_path);
    }
    prepare_context_layout(context)?;
    eprintln!("created context {}", context);
    Ok(ctx_path)
}

/// Known-binary extensions skipped during directory walks. Plain-text adjacent formats
/// (json, yaml, toml, xml, md, csv) are intentionally absent: the text pipeline handles them.
const DENIED_EXTENSIONS: &[&str] = &[
    // images
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "tiff", "tif", "ico", "heic", "avif", "psd",
    // audio / video
    "mp3", "wav", "flac", "ogg", "m4a", "aac", "mp4", "mov", "avi", "mkv", "webm", "wmv",
    // archives
    "zip", "tar", "gz", "tgz", "bz2", "xz", "7z", "rar", "z",
    // binaries / objects / packages
    "exe", "dll", "so", "dylib", "o", "a", "lib", "class", "jar", "war", "ear", "wasm", "pyc",
    "pyo", "bin", "deb", "rpm", "dmg", "iso", "apk", "ipa", // fonts
    "ttf", "otf", "woff", "woff2", "eot", // databases
    "db", "sqlite", "sqlite3", "mdb", "accdb",
];

/// Outcome of the pre-walk skip check. Callers distinguish denylist hits (worth counting in the
/// summary) from VCS/junk paths (normal walk hygiene, uncounted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkipCheck {
    Keep,
    InfrastructureDir,
    JunkFile,
    DeniedExtension,
}

pub(crate) fn classify_path(path: &Path) -> SkipCheck {
    use std::ffi::OsStr;

    if path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(".git" | ".ctx" | "target" | "node_modules")
        )
    }) {
        return SkipCheck::InfrastructureDir;
    }

    if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
        if DENIED_EXTENSIONS
            .iter()
            .any(|known| ext.eq_ignore_ascii_case(known))
        {
            return SkipCheck::DeniedExtension;
        }
    }

    match path.file_name() {
        Some(name) if name == OsStr::new(".DS_Store") || name == OsStr::new("Thumbs.db") => {
            SkipCheck::JunkFile
        }
        _ => SkipCheck::Keep,
    }
}

#[cfg(test)]
fn should_skip_path(path: &Path) -> bool {
    !matches!(classify_path(path), SkipCheck::Keep)
}

pub(crate) fn parse_layer(value: &str) -> Result<ContentLayer> {
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

pub(crate) fn hash_file(path: &Path) -> Result<String> {
    Ok(hash_bytes(&fs::read(path)?))
}

/// Streaming SHA-256 of a file. Reads in 64 KiB blocks so even multi-gigabyte files hash
/// without ballooning memory.
pub(crate) fn hash_file_streaming(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("read {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SourceStat {
    size: u64,
    mtime_unix: i64,
}

pub(crate) fn source_stat_from_metadata(metadata: &fs::Metadata) -> SourceStat {
    let mtime_unix = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    SourceStat {
        size: metadata.len(),
        mtime_unix,
    }
}

/// Fast path for large streamed files: when `(size, mtime)` and manifest/index state all match,
/// skip re-hashing and decoding.
pub(crate) fn already_fully_indexed_by_stat(
    ctx_path: &Path,
    root_str: &str,
    source_rel_str: &str,
    source_stat: SourceStat,
    with_content: bool,
) -> Result<bool> {
    let manifest = Manifest::load(ctx_path)?;
    let raw_enabled = manifest.config.store_raw_content || with_content;
    if let Some(source) = manifest.sources.iter().find(|s| s.root == root_str) {
        let entries: Vec<&ManifestEntry> = source
            .files
            .iter()
            .filter(|entry| entry.effective_source_path() == source_rel_str)
            .collect();
        if !entries.is_empty()
            && entries.iter().all(|entry| {
                let mtime = entry
                    .extra
                    .get("source_mtime_unix")
                    .and_then(Value::as_i64)
                    .unwrap_or(i64::MIN);
                let size = entry
                    .extra
                    .get("source_size")
                    .and_then(Value::as_u64)
                    .unwrap_or(u64::MAX);
                entry.hash == entry.hash_at_index
                    && size == source_stat.size
                    && mtime == source_stat.mtime_unix
                    && (!raw_enabled || entry.blob_ref.is_some())
            })
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexFilePlan {
    NeedsWork { size: u64 },
    SkipAlreadyIndexed,
    SkipTooLarge { size: u64 },
}

/// Cheap manifest/hash probe matching [`ingest_source_from_path`] skip rules (no decode).
pub(crate) fn probe_index_file(
    ctx_path: &Path,
    root_str: &str,
    source_rel: &Path,
    abs: &Path,
    with_content: bool,
) -> Result<IndexFilePlan> {
    let metadata = fs::metadata(abs)?;
    let size = metadata.len();
    let source_stat = source_stat_from_metadata(&metadata);
    let rel_str = source_rel.display().to_string();
    let stream = should_stream_plain_text(abs, source_rel, size);

    if !stream && size > extraction::decoder::MAX_BINARY_DECODER_BYTES {
        return Ok(IndexFilePlan::SkipTooLarge { size });
    }

    if stream {
        if already_fully_indexed_by_stat(ctx_path, root_str, &rel_str, source_stat, with_content)? {
            return Ok(IndexFilePlan::SkipAlreadyIndexed);
        }
    } else {
        let hash = hash_file(abs)?;
        if already_fully_indexed(ctx_path, root_str, &rel_str, &hash, with_content)? {
            return Ok(IndexFilePlan::SkipAlreadyIndexed);
        }
    }
    Ok(IndexFilePlan::NeedsWork { size })
}

/// Decide whether `(abs_path, size)` should be ingested via the streaming plain-text path. We
/// peek at [`PEEK_SNIFF_BYTES`] of content to let binary decoders claim before we commit to
/// streaming.
pub(crate) fn should_stream_plain_text(abs_path: &Path, rel: &Path, size: u64) -> bool {
    if size <= PLAIN_TEXT_STREAM_THRESHOLD {
        return false;
    }
    let Ok(mut file) = File::open(abs_path) else {
        return false;
    };
    let mut peek = vec![0u8; PEEK_SNIFF_BYTES.min(size as usize)];
    let read = match file.read(&mut peek) {
        Ok(n) => n,
        Err(_) => return false,
    };
    peek.truncate(read);
    !any_binary_decoder_claims(rel, &peek)
}

/// Pre-categorized ingest outcome. Batch callers use the discriminator to bucket skips into the
/// correct [`IngestionSummary`] counter without having to sniff error messages themselves.
pub(crate) enum IngestOutcomeKind {
    Decoded {
        outcome: AddOutcome,
        bytes_read: u64,
    },
    TooLarge {
        size: u64,
    },
    ReadError(anyhow::Error),
    DecodeError(anyhow::Error),
    EncodingError(anyhow::Error),
}

/// Dispatches a filesystem-backed source file through either the in-memory or streaming ingest
/// path based on size and peek-sniff. Returns a categorized outcome so batch callers can
/// attribute skips without error-message sniffing.
pub(crate) async fn ingest_source_from_path(
    ctx_path: &Path,
    root: &Path,
    source_rel: &Path,
    abs: &Path,
    layer: Option<ContentLayer>,
    with_content: bool,
) -> IngestOutcomeKind {
    let metadata = match fs::metadata(abs) {
        Ok(m) => m,
        Err(err) => {
            return IngestOutcomeKind::ReadError(
                anyhow::Error::new(err).context(format!("stat {}", abs.display())),
            );
        }
    };
    let size = metadata.len();
    let source_stat = source_stat_from_metadata(&metadata);
    let stream = should_stream_plain_text(abs, source_rel, size);

    if !stream && size > extraction::decoder::MAX_BINARY_DECODER_BYTES {
        return IngestOutcomeKind::TooLarge { size };
    }

    let result = if stream {
        add_source_file_streamed(
            ctx_path,
            root,
            source_rel,
            abs,
            source_stat,
            layer,
            with_content,
        )
        .await
    } else {
        match fs::read(abs) {
            Ok(bytes) => {
                add_source_file(ctx_path, root, source_rel, &bytes, layer, with_content).await
            }
            Err(err) => {
                return IngestOutcomeKind::ReadError(
                    anyhow::Error::new(err).context(format!("failed to read {}", abs.display())),
                );
            }
        }
    };

    match result {
        Ok(outcome) => IngestOutcomeKind::Decoded {
            outcome,
            bytes_read: size,
        },
        Err(err) => categorize_ingest_error(err),
    }
}

/// Inspect an ingest error and bucket it into one of the summary skip categories. Encoding
/// failures come from `PlainTextDecoder` with messages like "failed to decode as utf-8 text";
/// anything else counts as a generic decoder failure.
fn categorize_ingest_error(err: anyhow::Error) -> IngestOutcomeKind {
    let message = format!("{err:#}").to_lowercase();
    if message.contains("decode as utf-") {
        IngestOutcomeKind::EncodingError(err)
    } else {
        IngestOutcomeKind::DecodeError(err)
    }
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
pub const DRIFT_HINT: &str = "context may be stale; run `ctx update` to re-index drifted files";

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
    drifted_files.sort_by(|a, b| {
        (a.root.as_str(), a.path.as_str()).cmp(&(b.root.as_str(), b.path.as_str()))
    });
    Ok(DriftState {
        drift_detected: !drifted_files.is_empty(),
        drifted_files,
    })
}

// -------------------------------- integrity verification --------------------------------

/// Result of verifying a single blob-backed manifest entry (spec §13.1, §13.5).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum IntegrityStatus {
    Ok,
    Tampered,
    Missing,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VerifiedEntry {
    pub path: String,
    pub blob_ref: String,
    pub status: IntegrityStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrphanBlob {
    pub blob_hash: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VerifyReport {
    pub entries: Vec<VerifiedEntry>,
    pub orphans: Vec<OrphanBlob>,
    pub has_failures: bool,
}

/// Walks every manifest entry with a `blob_ref`, re-hashing the on-disk blob via
/// [`artifact::blob::read_verified`]. Also lists any file in `blobs/sha256/` that
/// no entry references. Pure reporting: never mutates disk or manifest.
pub fn verify_context(name: &str) -> Result<VerifyReport> {
    let ctx_path = open_existing_context(name)?;
    let manifest = Manifest::load(&ctx_path)?;

    let mut entries: Vec<VerifiedEntry> = Vec::new();
    let mut referenced: std::collections::HashSet<String> = std::collections::HashSet::new();

    for source in &manifest.sources {
        for entry in &source.files {
            let Some(blob_ref) = entry.blob_ref.as_deref() else {
                continue;
            };
            referenced.insert(blob_ref.trim_start_matches("sha256:").to_string());
            let verified =
                match artifact::blob::read_verified(&ctx_path, blob_ref, &entry.hash_at_index) {
                    Ok(_) => VerifiedEntry {
                        path: entry.path.clone(),
                        blob_ref: blob_ref.to_string(),
                        status: IntegrityStatus::Ok,
                        reason: None,
                    },
                    Err(artifact::blob::IntegrityError::Missing(_)) => VerifiedEntry {
                        path: entry.path.clone(),
                        blob_ref: blob_ref.to_string(),
                        status: IntegrityStatus::Missing,
                        reason: None,
                    },
                    Err(err @ artifact::blob::IntegrityError::BlobDigest { .. })
                    | Err(err @ artifact::blob::IntegrityError::ContentDigest { .. }) => {
                        VerifiedEntry {
                            path: entry.path.clone(),
                            blob_ref: blob_ref.to_string(),
                            status: IntegrityStatus::Tampered,
                            reason: Some(err.to_string()),
                        }
                    }
                    Err(err @ artifact::blob::IntegrityError::Io(_)) => VerifiedEntry {
                        path: entry.path.clone(),
                        blob_ref: blob_ref.to_string(),
                        status: IntegrityStatus::Missing,
                        reason: Some(err.to_string()),
                    },
                };
            entries.push(verified);
        }
    }

    let mut orphans: Vec<OrphanBlob> = Vec::new();
    let blobs_dir = blobs_path(&ctx_path);
    if blobs_dir.exists() {
        for dir_entry in
            fs::read_dir(&blobs_dir).with_context(|| format!("read {}", blobs_dir.display()))?
        {
            let dir_entry = dir_entry?;
            if !dir_entry.file_type()?.is_file() {
                continue;
            }
            let name = dir_entry.file_name().to_string_lossy().into_owned();
            if !referenced.contains(&name) {
                orphans.push(OrphanBlob {
                    blob_hash: format!("sha256:{name}"),
                });
            }
        }
    }
    orphans.sort_by(|a, b| a.blob_hash.cmp(&b.blob_hash));

    let has_failures = entries.iter().any(|e| {
        matches!(
            e.status,
            IntegrityStatus::Tampered | IntegrityStatus::Missing
        )
    });

    Ok(VerifyReport {
        entries,
        orphans,
        has_failures,
    })
}

// -------------------------------- notes --------------------------------

/// Read-only snapshot of notes content for inclusion in query responses.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NotesSummary {
    pub index: Option<String>,
    pub summary: Option<String>,
    pub topics: Vec<String>,
}

/// A single notes file read off disk, paired with its current hash.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NotesContent {
    pub path: String,
    pub content: String,
    pub hash: String,
}

/// Write modes for `write_notes_file`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotesWriteMode {
    Replace,
    Append,
}

impl NotesWriteMode {
    pub fn parse(value: Option<&str>) -> Result<Self> {
        match value.unwrap_or("replace") {
            "replace" => Ok(Self::Replace),
            "append" => Ok(Self::Append),
            other => bail!("unknown notes write mode {}", other),
        }
    }
}

/// Outcome of a successful notes writeback.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NotesWriteOutcome {
    pub path: String,
    pub hash: String,
}

/// Rescans `notes/` and rebuilds the manifest's notes registry. `updated_at` is preserved
/// for entries whose hash did not change; new or changed entries are stamped with `now`.
pub fn refresh_notes_registry(ctx_path: &Path, manifest: &mut Manifest) -> Result<()> {
    migrate_legacy_notes_dir(ctx_path)?;
    let dir = notes_path(ctx_path);
    if !dir.exists() {
        manifest.notes.files.clear();
        return Ok(());
    }

    let existing: std::collections::HashMap<String, artifact::manifest::NoteFile> = manifest
        .notes
        .files
        .drain(..)
        .map(|entry| (entry.path.clone(), entry))
        .collect();

    let mut refreshed: Vec<artifact::manifest::NoteFile> = Vec::new();
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
        let bytes = fs::read(abs).with_context(|| format!("read notes file {}", abs.display()))?;
        let hash = hash_bytes(&bytes);
        let updated_at = match existing.get(&rel_str) {
            Some(prev) if prev.hash == hash => prev.updated_at,
            _ => Utc::now(),
        };
        refreshed.push(artifact::manifest::NoteFile {
            path: rel_str,
            hash,
            updated_at,
            extra: Default::default(),
        });
    }

    refreshed.sort_by(|a, b| a.path.cmp(&b.path));
    manifest.notes.files = refreshed;
    Ok(())
}

/// Reads index.md (first) and summary.md (second), plus the list of other notes/*.md paths.
pub fn read_notes_summary(ctx_path: &Path) -> Result<NotesSummary> {
    migrate_legacy_notes_dir(ctx_path)?;
    let dir = notes_path(ctx_path);
    let read_opt = |name: &str| -> Result<Option<String>> {
        let path = dir.join(name);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(
            fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?,
        ))
    };
    let index = read_opt("index.md")?;
    let summary = read_opt("summary.md")?;

    let mut topics: Vec<String> = Vec::new();
    if dir.exists() {
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
            let name = abs.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "index.md" || name == "summary.md" {
                continue;
            }
            let rel = abs.strip_prefix(ctx_path).unwrap_or(abs);
            topics.push(
                rel.to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/"),
            );
        }
    }
    topics.sort();

    Ok(NotesSummary {
        index,
        summary,
        topics,
    })
}

/// Reads a single notes file. `rel_path` must start with `notes/` and must resolve inside
/// the artifact's notes directory.
pub fn read_notes_file(ctx_path: &Path, rel_path: &str) -> Result<NotesContent> {
    let canonical = canonical_notes_path(rel_path);
    let abs = resolve_notes_path(ctx_path, &canonical)?;
    if !abs.exists() {
        bail!("notes file {} does not exist", canonical);
    }
    let bytes = fs::read(&abs).with_context(|| format!("read {}", abs.display()))?;
    let hash = hash_bytes(&bytes);
    let content = String::from_utf8(bytes)
        .with_context(|| format!("notes file {} is not valid utf-8", canonical))?;
    Ok(NotesContent {
        path: canonical,
        content,
        hash,
    })
}

/// Writes (or appends to) a notes file and updates the manifest registry entry.
pub fn write_notes_file(
    context: &str,
    rel_path: &str,
    content: &str,
    mode: NotesWriteMode,
) -> Result<NotesWriteOutcome> {
    let ctx_path = open_existing_context(context)?;
    let canonical = canonical_notes_path(rel_path);
    let abs = resolve_notes_path(&ctx_path, &canonical)?;

    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent)?;
    }

    match mode {
        NotesWriteMode::Replace => {
            fs::write(&abs, content).with_context(|| format!("write {}", abs.display()))?;
        }
        NotesWriteMode::Append => {
            let mut buffer = if abs.exists() {
                fs::read(&abs).with_context(|| format!("read {}", abs.display()))?
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

    let mut manifest = Manifest::load(&ctx_path)?;
    manifest.upsert_note(&canonical, &hash);
    manifest.save(&ctx_path)?;

    Ok(NotesWriteOutcome {
        path: canonical,
        hash,
    })
}

/// Resolves `rel_path` to an absolute path inside `ctx_path/notes/`, rejecting any attempt
/// to escape the notes directory.
fn resolve_notes_path(ctx_path: &Path, rel_path: &str) -> Result<PathBuf> {
    let normalized = normalize_notes_path(rel_path);
    if !normalized.starts_with("notes/") {
        bail!("notes path must begin with \"notes/\"");
    }
    if normalized.split('/').any(|segment| segment == "..") {
        bail!("notes path must not contain \"..\" segments");
    }
    let abs = ctx_path.join(&normalized);
    let notes_root = notes_path(ctx_path);
    let check_base = notes_root.canonicalize().unwrap_or(notes_root.clone());
    // Walk up until we find an existing ancestor we can canonicalize; this lets us
    // validate paths whose leaf directory has not been created yet (e.g. notes/topics/).
    let check_target = abs
        .ancestors()
        .find_map(|anc| anc.canonicalize().ok())
        .unwrap_or_else(|| abs.clone());
    if !check_target.starts_with(&check_base) {
        bail!("notes path {} escapes the notes directory", rel_path);
    }
    Ok(abs)
}

fn normalize_notes_path(rel_path: &str) -> String {
    rel_path.replace('\\', "/")
}

/// Canonicalizes a notes-relative path so root-level topic writes land under
/// `notes/topics/`. Reserved files (`notes/summary.md`, `notes/index.md`) and paths
/// that already include a subdirectory are returned unchanged.
pub(crate) fn canonical_notes_path(rel_path: &str) -> String {
    let normalized = normalize_notes_path(rel_path);
    let normalized = normalized.replacen("aura/", "notes/", 1);
    let segments: Vec<&str> = normalized.split('/').collect();
    if segments.len() != 2 || segments[0] != "notes" {
        return normalized;
    }
    let name = segments[1];
    if name == "aura.md" {
        return String::from("notes/summary.md");
    }
    if name == "summary.md" || name == "index.md" {
        return normalized;
    }
    format!("notes/topics/{name}")
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

    pub(crate) fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    pub(crate) struct MockOpenAiServer {
        base_url: String,
        shutdown: Arc<AtomicBool>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl MockOpenAiServer {
        pub(crate) fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock openai");
            listener
                .set_nonblocking(true)
                .expect("mock openai nonblocking");
            let addr = listener.local_addr().expect("mock openai addr");
            let shutdown = Arc::new(AtomicBool::new(false));
            let shutdown_flag = shutdown.clone();
            let handle = thread::spawn(move || {
                while !shutdown_flag.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => handle_mock_openai_connection(stream),
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            });

            Self {
                base_url: format!("http://{addr}/v1"),
                shutdown,
                handle: Some(handle),
            }
        }

        pub(crate) fn base_url(&self) -> &str {
            &self.base_url
        }
    }

    impl Drop for MockOpenAiServer {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::Relaxed);
            let _ = TcpStream::connect(
                self.base_url
                    .trim_start_matches("http://")
                    .trim_end_matches("/v1"),
            );
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    fn handle_mock_openai_connection(mut stream: TcpStream) {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 4096];
        let mut header_end = None;
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    buffer.extend_from_slice(&chunk[..n]);
                    if let Some(pos) = find_header_end(&buffer) {
                        header_end = Some(pos);
                        break;
                    }
                }
                Err(_) => return,
            }
        }

        let Some(header_end) = header_end else {
            return;
        };
        let headers = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.split_once(':').and_then(|(name, value)| {
                    if name.eq_ignore_ascii_case("content-length") {
                        value.trim().parse::<usize>().ok()
                    } else {
                        None
                    }
                })
            })
            .unwrap_or(0);

        let body_start = header_end + 4;
        while buffer.len().saturating_sub(body_start) < content_length {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => buffer.extend_from_slice(&chunk[..n]),
                Err(_) => return,
            }
        }

        let request_line = headers.lines().next().unwrap_or_default();
        let path = request_line
            .split_whitespace()
            .nth(1)
            .unwrap_or_default()
            .to_string();
        let body = String::from_utf8_lossy(&buffer[body_start..]).to_string();
        let response = mock_openai_response(&path, &body);
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    fn mock_openai_response(path: &str, body: &str) -> String {
        let value = if path.ends_with("/chat/completions") {
            mock_chat_completion(body)
        } else if path.ends_with("/embeddings") {
            mock_embeddings(body)
        } else {
            serde_json::json!({ "error": "not found" })
        };

        let status = if path.ends_with("/chat/completions") || path.ends_with("/embeddings") {
            "HTTP/1.1 200 OK"
        } else {
            "HTTP/1.1 404 Not Found"
        };
        let body = value.to_string();
        format!(
            "{status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    }

    fn mock_chat_completion(body: &str) -> serde_json::Value {
        let request: serde_json::Value =
            serde_json::from_str(body).expect("mock openai chat request json");
        let prompt = request["messages"][0]["content"]
            .as_str()
            .unwrap_or_default();
        let content = if prompt.contains("\"citations\"") && prompt.contains("Retrieved results:") {
            mock_query_answer_json(prompt)
        } else if prompt.contains("\"task_description\"") {
            mock_procedural_json(prompt)
        } else {
            mock_semantic_json(prompt)
        };

        serde_json::json!({
            "choices": [{
                "message": {
                    "content": content
                }
            }]
        })
    }

    fn mock_query_answer_json(prompt: &str) -> String {
        let answer = if prompt.to_ascii_lowercase().contains("rs256") {
            "AuthService uses RS256."
        } else if prompt.to_ascii_lowercase().contains("staging") {
            "The retrieved results describe a staging deployment flow."
        } else {
            "The retrieved results contain a direct answer."
        };

        serde_json::json!({
            "answer": answer,
            "citations": [1],
        })
        .to_string()
    }

    fn mock_embeddings(body: &str) -> serde_json::Value {
        let request: serde_json::Value =
            serde_json::from_str(body).expect("mock openai embeddings request json");
        let inputs = request["input"].as_array().cloned().unwrap_or_default();
        let data: Vec<serde_json::Value> = inputs
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let text = value.as_str().unwrap_or_default();
                serde_json::json!({
                    "index": index,
                    "embedding": mock_embedding(text),
                })
            })
            .collect();
        serde_json::json!({ "data": data })
    }

    fn mock_semantic_json(prompt: &str) -> String {
        let content = prompt
            .rsplit("\ncontent:\n")
            .next()
            .unwrap_or(prompt)
            .trim();
        let summary = content.lines().next().unwrap_or(content).trim().to_string();
        let entities: Vec<String> = content
            .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
            .filter(|term| {
                term.len() > 2
                    && term
                        .chars()
                        .next()
                        .map(|ch| ch.is_ascii_uppercase())
                        .unwrap_or(false)
            })
            .map(str::to_string)
            .collect();
        let triples = if content.contains(" uses ") {
            vec![serde_json::json!(["AuthService", "uses", "RS256"])]
        } else {
            Vec::new()
        };

        serde_json::json!({
            "summary": summary,
            "facts": [content],
            "entities": entities,
            "triples": triples,
        })
        .to_string()
    }

    fn mock_procedural_json(prompt: &str) -> String {
        let content = prompt
            .rsplit("\ncontent:\n")
            .next()
            .unwrap_or(prompt)
            .trim();
        let steps: Vec<String> = content
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                let digits = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
                if digits == 0 {
                    return None;
                }
                let rest = trimmed[digits..].trim_start_matches('.').trim();
                if rest.is_empty() {
                    None
                } else {
                    Some(rest.to_string())
                }
            })
            .collect();
        let outcome = if content.to_ascii_lowercase().contains("success") {
            "success"
        } else {
            "partial"
        };

        serde_json::json!({
            "task_description": content.lines().next().unwrap_or("procedure"),
            "steps": steps,
            "outcome": outcome,
            "failure_modes": [],
            "context": { "language": null, "framework": null, "environment": null },
            "confidence": 0.9,
        })
        .to_string()
    }

    fn mock_embedding(text: &str) -> Vec<f32> {
        let dims = 32usize;
        let mut vector = vec![0.0_f32; dims];
        for token in text
            .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
            .map(|token| token.trim().to_ascii_lowercase())
            .filter(|token| !token.is_empty())
        {
            let mut hash = 1469598103934665603u64;
            for byte in token.as_bytes() {
                hash ^= *byte as u64;
                hash = hash.wrapping_mul(1099511628211);
            }
            let index = (hash as usize) % dims;
            let sign = if (hash >> 8) & 1 == 0 { 1.0 } else { -1.0 };
            vector[index] += sign;
        }

        let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
        if norm > 0.0 {
            for value in &mut vector {
                *value /= norm;
            }
        }

        vector
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn should_skip_denied_extensions() {
        for ext in [
            "jpg", "JPEG", "png", "mp4", "mp3", "zip", "tar.gz", "exe", "dll", "so", "wasm", "ttf",
            "woff2", "sqlite",
        ] {
            // For multi-dot extensions (tar.gz), Path::extension returns only the final piece.
            let leaf = format!("asset.{ext}");
            let path = Path::new(&leaf);
            assert!(
                should_skip_path(path),
                "expected .{ext} to be skipped ({})",
                path.display()
            );
        }
    }

    #[test]
    fn should_not_skip_indexable_extensions() {
        for name in [
            "README.md",
            "notes.txt",
            "data.json",
            "config.yaml",
            "pyproject.toml",
            "feed.xml",
            "table.csv",
            "report.pdf",
            "book.epub",
            "notebook.ipynb",
            "deck.pptx",
            "memo.docx",
            "style.rtf",
            "page.html",
            "sheet.xlsx",
            "lib.rs",
            "main.py",
        ] {
            let path = Path::new(name);
            assert!(
                !should_skip_path(path),
                "expected {name} to pass the denylist"
            );
        }
    }

    #[test]
    fn hash_file_streaming_matches_slurp_hash() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("payload.bin");
        let data: Vec<u8> = (0u8..=255).cycle().take(200_000).collect();
        fs::write(&path, &data).expect("write payload");
        let streamed = hash_file_streaming(&path).expect("stream hash");
        let slurped = hash_file(&path).expect("slurp hash");
        assert_eq!(streamed, slurped);
    }

    #[test]
    fn streaming_path_produces_ordered_virtual_chunks() {
        let src_dir = TempDir::new().expect("src dir");
        let file_path = src_dir.path().join("big.log");
        let target_bytes = (crate::extraction::decoder::PLAIN_TEXT_STREAM_THRESHOLD as usize)
            + (2 * crate::extraction::decoder::PLAIN_TEXT_UNIT_BYTES);
        let line = "streaming line payload with deterministic content\n";
        let mut content = String::with_capacity(target_bytes + line.len());
        while content.len() < target_bytes {
            content.push_str(line);
        }
        fs::write(&file_path, content.as_bytes()).expect("write large source");
        let metadata = fs::metadata(&file_path).expect("metadata");
        assert!(should_stream_plain_text(
            &file_path,
            Path::new("big.log"),
            metadata.len()
        ));

        let file = File::open(&file_path).expect("open");
        let mut stream = PlainTextUnitStream::new(BufReader::new(file));
        let mut chunk_paths: Vec<PathBuf> = Vec::new();
        while let Some(unit) = stream.next_unit().expect("stream") {
            chunk_paths.push(unit.virtual_path);
        }

        assert!(
            chunk_paths.len() >= 2,
            "expected multiple streamed virtual units, got {}",
            chunk_paths.len()
        );
        for (idx, path) in chunk_paths.iter().enumerate() {
            assert_eq!(path, &PathBuf::from(format!("chunk-{:0>4}.txt", idx + 1)));
        }
    }

    #[test]
    fn stat_fast_path_accepts_matching_manifest_markers() {
        let tempdir = TempDir::new().expect("tempdir");
        let ctx_path = tempdir.path();
        fs::create_dir_all(ctx_path).expect("mkdir ctx");

        let root = String::from("/tmp/source-root");
        let source_rel = String::from("big.log");
        let size = 9 * 1024 * 1024u64;
        let mtime_unix = 1_715_000_000i64;

        let mut manifest = Manifest::empty("stat-fast-path");
        let source = manifest.upsert_source(&root);
        let mut entry = ManifestEntry {
            path: String::from("big.log/chunk-0001.txt"),
            hash: String::from("sha256:abc"),
            hash_at_index: String::from("sha256:abc"),
            indexed_at: Utc::now(),
            r#type: String::from("semantic"),
            blob_ref: None,
            source_path: Some(source_rel.clone()),
            extra: serde_json::Map::new(),
        };
        entry
            .extra
            .insert(String::from("source_size"), Value::from(size));
        entry
            .extra
            .insert(String::from("source_mtime_unix"), Value::from(mtime_unix));
        source.files.push(entry);
        manifest.save(ctx_path).expect("save manifest");

        let matches = already_fully_indexed_by_stat(
            ctx_path,
            &root,
            &source_rel,
            SourceStat { size, mtime_unix },
            false,
        )
        .expect("fast-path check");
        assert!(matches, "expected stat fast-path to match");
    }

    #[test]
    fn stat_fast_path_rejects_mismatched_markers() {
        let tempdir = TempDir::new().expect("tempdir");
        let ctx_path = tempdir.path();
        fs::create_dir_all(ctx_path).expect("mkdir ctx");

        let root = String::from("/tmp/source-root");
        let source_rel = String::from("big.log");
        let size = 9 * 1024 * 1024u64;
        let mtime_unix = 1_715_000_000i64;

        let mut manifest = Manifest::empty("stat-fast-path-mismatch");
        let source = manifest.upsert_source(&root);
        let mut entry = ManifestEntry {
            path: String::from("big.log/chunk-0001.txt"),
            hash: String::from("sha256:abc"),
            hash_at_index: String::from("sha256:abc"),
            indexed_at: Utc::now(),
            r#type: String::from("semantic"),
            blob_ref: None,
            source_path: Some(source_rel.clone()),
            extra: serde_json::Map::new(),
        };
        entry
            .extra
            .insert(String::from("source_size"), Value::from(size));
        entry
            .extra
            .insert(String::from("source_mtime_unix"), Value::from(mtime_unix));
        source.files.push(entry);
        manifest.save(ctx_path).expect("save manifest");

        let wrong_size = already_fully_indexed_by_stat(
            ctx_path,
            &root,
            &source_rel,
            SourceStat {
                size: size + 1,
                mtime_unix,
            },
            false,
        )
        .expect("fast-path check");
        assert!(!wrong_size, "size mismatch must bypass stat fast-path");

        let wrong_mtime = already_fully_indexed_by_stat(
            ctx_path,
            &root,
            &source_rel,
            SourceStat {
                size,
                mtime_unix: mtime_unix + 1,
            },
            false,
        )
        .expect("fast-path check");
        assert!(!wrong_mtime, "mtime mismatch must bypass stat fast-path");
    }

    #[test]
    fn should_skip_junk_and_vcs() {
        assert!(should_skip_path(Path::new("project/.git/HEAD")));
        assert!(should_skip_path(Path::new("project/target/debug/app")));
        assert!(should_skip_path(Path::new(
            "project/node_modules/pkg/index.js"
        )));
        assert!(should_skip_path(Path::new(".DS_Store")));
        assert!(should_skip_path(Path::new("docs/Thumbs.db")));
    }

    #[tokio::test]
    async fn inline_semantic_round_trip() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let tempdir = TempDir::new().expect("tempdir");
        let home_root = TempDir::new().expect("home root");
        let server = crate::test_support::MockOpenAiServer::start();

        let saved_openai = std::env::var("OPENAI_API_KEY").ok();
        let saved_openai_base = std::env::var("CTX_OPENAI_BASE_URL").ok();

        std::env::set_var("HOME", home_root.path());
        std::env::set_var("CTX_PATH", tempdir.path());
        std::env::set_var("OPENAI_API_KEY", "test-openai-key");
        std::env::set_var("CTX_OPENAI_BASE_URL", server.base_url());

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

        std::env::remove_var("CTX_PATH");
        std::env::remove_var("HOME");
        restore_optional_env("OPENAI_API_KEY", saved_openai.as_deref());
        restore_optional_env("CTX_OPENAI_BASE_URL", saved_openai_base.as_deref());
    }

    #[tokio::test]
    async fn inline_procedural_round_trip() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let tempdir = TempDir::new().expect("tempdir");
        let home_root = TempDir::new().expect("home root");
        let server = crate::test_support::MockOpenAiServer::start();

        let saved_openai = std::env::var("OPENAI_API_KEY").ok();
        let saved_openai_base = std::env::var("CTX_OPENAI_BASE_URL").ok();

        std::env::set_var("HOME", home_root.path());
        std::env::set_var("CTX_PATH", tempdir.path());
        std::env::set_var("OPENAI_API_KEY", "test-openai-key");
        std::env::set_var("CTX_OPENAI_BASE_URL", server.base_url());

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
            result.content.contains("deploy") || result.summary.to_lowercase().contains("staging")
        }));

        std::env::remove_var("CTX_PATH");
        std::env::remove_var("HOME");
        restore_optional_env("OPENAI_API_KEY", saved_openai.as_deref());
        restore_optional_env("CTX_OPENAI_BASE_URL", saved_openai_base.as_deref());
    }

    fn restore_optional_env(key: &str, value: Option<&str>) {
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    struct NotesTestEnv {
        _tempdir: TempDir,
        _home_root: TempDir,
        _server: crate::test_support::MockOpenAiServer,
        saved_openai: Option<String>,
        saved_openai_base: Option<String>,
    }

    impl Drop for NotesTestEnv {
        fn drop(&mut self) {
            std::env::remove_var("CTX_PATH");
            std::env::remove_var("HOME");
            restore_optional_env("OPENAI_API_KEY", self.saved_openai.as_deref());
            restore_optional_env("CTX_OPENAI_BASE_URL", self.saved_openai_base.as_deref());
        }
    }

    fn setup_notes_env() -> NotesTestEnv {
        let tempdir = TempDir::new().expect("tempdir");
        let home_root = TempDir::new().expect("home root");
        let server = crate::test_support::MockOpenAiServer::start();

        let saved_openai = std::env::var("OPENAI_API_KEY").ok();
        let saved_openai_base = std::env::var("CTX_OPENAI_BASE_URL").ok();

        std::env::set_var("HOME", home_root.path());
        std::env::set_var("CTX_PATH", tempdir.path());
        std::env::set_var("OPENAI_API_KEY", "test-openai-key");
        std::env::set_var("CTX_OPENAI_BASE_URL", server.base_url());

        NotesTestEnv {
            _tempdir: tempdir,
            _home_root: home_root,
            _server: server,
            saved_openai,
            saved_openai_base,
        }
    }

    #[tokio::test]
    async fn notes_seeded_on_init() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_notes_env();

        init_context("notes-seed").await.expect("init context");
        let ctx_path = open_existing_context("notes-seed").expect("open context");

        let notes_dir = notes_path(&ctx_path);
        assert!(notes_dir.join("index.md").exists());
        assert!(notes_dir.join("summary.md").exists());

        let manifest = Manifest::load(&ctx_path).expect("load manifest");
        let paths: Vec<&str> = manifest
            .notes
            .files
            .iter()
            .map(|entry| entry.path.as_str())
            .collect();
        assert!(paths.contains(&"notes/index.md"));
        assert!(paths.contains(&"notes/summary.md"));
        for entry in &manifest.notes.files {
            assert!(!entry.hash.is_empty());
        }
    }

    #[tokio::test]
    async fn init_context_requires_openai_key() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let tempdir = TempDir::new().expect("tempdir");
        let home_root = TempDir::new().expect("home root");

        let saved_openai = std::env::var("OPENAI_API_KEY").ok();
        let saved_openai_base = std::env::var("CTX_OPENAI_BASE_URL").ok();

        std::env::set_var("HOME", home_root.path());
        std::env::set_var("CTX_PATH", tempdir.path());
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("CTX_OPENAI_BASE_URL");

        let error = init_context("missing-key")
            .await
            .expect_err("missing api key");
        assert!(error.to_string().contains("OPENAI_API_KEY is required"));

        std::env::remove_var("CTX_PATH");
        std::env::remove_var("HOME");
        restore_optional_env("OPENAI_API_KEY", saved_openai.as_deref());
        restore_optional_env("CTX_OPENAI_BASE_URL", saved_openai_base.as_deref());
    }

    #[tokio::test]
    async fn notes_writeback_round_trip() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_notes_env();

        init_context("notes-write").await.expect("init context");

        let outcome = write_notes_file(
            "notes-write",
            "notes/auth.md",
            "# Auth notes\n\nRS256 tokens only.\n",
            NotesWriteMode::Replace,
        )
        .expect("write notes file");
        assert_eq!(outcome.path, "notes/topics/auth.md");
        assert!(!outcome.hash.is_empty());

        let ctx_path = open_existing_context("notes-write").expect("open context");
        assert!(ctx_path.join("notes/topics/auth.md").exists());
        assert!(!ctx_path.join("notes/auth.md").exists());
        let content = read_notes_file(&ctx_path, "notes/auth.md").expect("read notes file");
        assert!(content.content.contains("RS256 tokens only"));
        assert_eq!(content.hash, outcome.hash);

        let manifest = Manifest::load(&ctx_path).expect("load manifest");
        let entry = manifest
            .notes
            .files
            .iter()
            .find(|entry| entry.path == "notes/topics/auth.md")
            .expect("manifest entry");
        assert_eq!(entry.hash, outcome.hash);

        write_notes_file(
            "notes-write",
            "notes/auth.md",
            "Additional note.\n",
            NotesWriteMode::Append,
        )
        .expect("append notes file");
        let appended = read_notes_file(&ctx_path, "notes/auth.md").expect("re-read");
        assert!(appended.content.contains("RS256 tokens only"));
        assert!(appended.content.contains("Additional note."));
    }

    #[tokio::test]
    async fn notes_read_tool_round_trip() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_notes_env();

        init_context("notes-read").await.expect("init context");
        write_notes_file(
            "notes-read",
            "notes/deploy.md",
            "deploy steps",
            NotesWriteMode::Replace,
        )
        .expect("seed topic file");

        let ctx_path = open_existing_context("notes-read").expect("open context");
        let summary = read_notes_summary(&ctx_path).expect("read summary");
        assert!(summary.index.is_some());
        assert!(summary.summary.is_some());
        assert!(summary
            .topics
            .contains(&String::from("notes/topics/deploy.md")));

        let content = read_notes_file(&ctx_path, "notes/deploy.md").expect("read file");
        assert_eq!(content.path, "notes/topics/deploy.md");
        assert_eq!(content.content, "deploy steps");
    }

    #[tokio::test]
    async fn drift_state_reads_manifest_only() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_notes_env();

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
        let _env = setup_notes_env();

        init_context("drift-status").await.expect("init context");
        let ctx_path = open_existing_context("drift-status").expect("open context");

        let src_dir = TempDir::new().expect("src dir");
        let file_path = src_dir.path().join("note.md");
        fs::write(&file_path, "original content").expect("write src");

        add_to_context(
            "drift-status",
            &file_path,
            Some(ContentLayer::Semantic),
            false,
        )
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
        let _env = setup_notes_env();

        init_context("drift-query").await.expect("init context");
        let ctx_path = open_existing_context("drift-query").expect("open context");

        let src_dir = TempDir::new().expect("src dir");
        let file_path = src_dir.path().join("note.md");
        fs::write(&file_path, "AuthService uses RS256 tokens.").expect("write src");

        add_to_context(
            "drift-query",
            &file_path,
            Some(ContentLayer::Semantic),
            false,
        )
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
        let _env = setup_notes_env();

        init_context("drift-update").await.expect("init context");
        let ctx_path = open_existing_context("drift-update").expect("open context");

        let src_dir = TempDir::new().expect("src dir");
        let file_path = src_dir.path().join("note.md");
        fs::write(&file_path, "initial").expect("write src");

        add_to_context(
            "drift-update",
            &file_path,
            Some(ContentLayer::Semantic),
            false,
        )
        .await
        .expect("add to context");

        fs::write(&file_path, "updated body").expect("mutate src");
        let status = context_status("drift-update").expect("status");
        assert_eq!(status.dirty_count, 1);
        assert!(drift_state(&ctx_path).expect("drift set").drift_detected);

        update_context("drift-update")
            .await
            .expect("update context");

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

    #[test]
    fn canonical_notes_path_redirects_root_topic() {
        assert_eq!(canonical_notes_path("notes/foo.md"), "notes/topics/foo.md");
        assert_eq!(canonical_notes_path("aura/foo.md"), "notes/topics/foo.md");
    }

    #[test]
    fn canonical_notes_path_preserves_reserved() {
        assert_eq!(canonical_notes_path("notes/summary.md"), "notes/summary.md");
        assert_eq!(canonical_notes_path("aura/aura.md"), "notes/summary.md");
        assert_eq!(canonical_notes_path("notes/index.md"), "notes/index.md");
    }

    #[test]
    fn canonical_notes_path_preserves_explicit_subdir() {
        assert_eq!(
            canonical_notes_path("notes/topics/foo.md"),
            "notes/topics/foo.md"
        );
        assert_eq!(
            canonical_notes_path("notes/scratch/foo.md"),
            "notes/scratch/foo.md"
        );
    }

    #[tokio::test]
    async fn notes_refresh_detects_external_edit() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_notes_env();

        init_context("notes-drift").await.expect("init context");
        let ctx_path = open_existing_context("notes-drift").expect("open context");

        let manifest_before = Manifest::load(&ctx_path).expect("load manifest");
        let before_hash = manifest_before
            .notes
            .files
            .iter()
            .find(|entry| entry.path == "notes/index.md")
            .expect("index entry")
            .hash
            .clone();

        let index_path = notes_path(&ctx_path).join("index.md");
        fs::write(&index_path, "# Notes Index\n\nEdited externally.\n").expect("external edit");

        update_context("notes-drift").await.expect("update context");

        let manifest_after = Manifest::load(&ctx_path).expect("reload manifest");
        let after = manifest_after
            .notes
            .files
            .iter()
            .find(|entry| entry.path == "notes/index.md")
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
        let _env = setup_notes_env();

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

        let blob_path =
            artifact::blobs_path(&ctx_path).join(blob_ref.trim_start_matches("sha256:"));
        assert!(
            blob_path.exists(),
            "blob file {} missing",
            blob_path.display()
        );
    }

    #[tokio::test]
    async fn add_without_with_content_leaves_blob_unset() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_notes_env();

        init_context("blob-default").await.expect("init context");
        let ctx_path = open_existing_context("blob-default").expect("open context");

        let src_dir = TempDir::new().expect("src dir");
        let file_path = src_dir.path().join("note.md");
        fs::write(&file_path, "hash only body").expect("write src");

        add_to_context(
            "blob-default",
            &file_path,
            Some(ContentLayer::Semantic),
            false,
        )
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
        let _env = setup_notes_env();

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
        let _env = setup_notes_env();

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

    #[tokio::test]
    async fn verify_reports_ok_for_clean_blobs() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_notes_env();

        init_context("verify-ok").await.expect("init context");

        let src_dir = TempDir::new().expect("src dir");
        let file_path = src_dir.path().join("note.md");
        fs::write(&file_path, "verified body").expect("write src");

        add_to_context("verify-ok", &file_path, Some(ContentLayer::Semantic), true)
            .await
            .expect("add with content");

        let report = verify_context("verify-ok").expect("verify context");
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].status, IntegrityStatus::Ok);
        assert!(report.orphans.is_empty());
        assert!(!report.has_failures);
    }

    #[tokio::test]
    async fn verify_detects_tampered_blob() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_notes_env();

        init_context("verify-tamper").await.expect("init context");
        let ctx_path = open_existing_context("verify-tamper").expect("open context");

        let src_dir = TempDir::new().expect("src dir");
        let file_path = src_dir.path().join("note.md");
        fs::write(&file_path, "original body").expect("write src");

        add_to_context(
            "verify-tamper",
            &file_path,
            Some(ContentLayer::Semantic),
            true,
        )
        .await
        .expect("add with content");

        let manifest = Manifest::load(&ctx_path).expect("load manifest");
        let blob_ref = manifest
            .sources
            .first()
            .and_then(|s| s.files.first())
            .and_then(|e| e.blob_ref.clone())
            .expect("blob_ref");
        let blob_path =
            artifact::blobs_path(&ctx_path).join(blob_ref.trim_start_matches("sha256:"));
        fs::write(&blob_path, b"not a valid zstd blob").expect("tamper blob");

        let report = verify_context("verify-tamper").expect("verify context");
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].status, IntegrityStatus::Tampered);
        assert!(report.has_failures);
    }

    #[tokio::test]
    async fn verify_detects_missing_blob() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_notes_env();

        init_context("verify-missing").await.expect("init context");
        let ctx_path = open_existing_context("verify-missing").expect("open context");

        let src_dir = TempDir::new().expect("src dir");
        let file_path = src_dir.path().join("note.md");
        fs::write(&file_path, "soon to vanish").expect("write src");

        add_to_context(
            "verify-missing",
            &file_path,
            Some(ContentLayer::Semantic),
            true,
        )
        .await
        .expect("add with content");

        let manifest = Manifest::load(&ctx_path).expect("load manifest");
        let blob_ref = manifest
            .sources
            .first()
            .and_then(|s| s.files.first())
            .and_then(|e| e.blob_ref.clone())
            .expect("blob_ref");
        let blob_path =
            artifact::blobs_path(&ctx_path).join(blob_ref.trim_start_matches("sha256:"));
        fs::remove_file(&blob_path).expect("remove blob");

        let report = verify_context("verify-missing").expect("verify context");
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].status, IntegrityStatus::Missing);
        assert!(report.has_failures);
    }

    #[tokio::test]
    async fn verify_reports_orphan_blobs() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let _env = setup_notes_env();

        init_context("verify-orphan").await.expect("init context");
        let ctx_path = open_existing_context("verify-orphan").expect("open context");

        let src_dir = TempDir::new().expect("src dir");
        let file_path = src_dir.path().join("note.md");
        fs::write(&file_path, "referenced body").expect("write src");

        add_to_context(
            "verify-orphan",
            &file_path,
            Some(ContentLayer::Semantic),
            true,
        )
        .await
        .expect("add with content");

        let orphan_hex = "0".repeat(64);
        let orphan_path = artifact::blobs_path(&ctx_path).join(&orphan_hex);
        fs::write(&orphan_path, b"stray bytes").expect("write orphan");

        let report = verify_context("verify-orphan").expect("verify context");
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].status, IntegrityStatus::Ok);
        assert_eq!(report.orphans.len(), 1);
        assert_eq!(report.orphans[0].blob_hash, format!("sha256:{orphan_hex}"));
        assert!(!report.has_failures);
    }
}
