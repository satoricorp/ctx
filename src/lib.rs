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

use artifact::{blobs_path, context_path, context_root, index_path, manifest_path, Manifest};
use index::procedural::{ingest_procedural_document, record_procedure_structured};
use index::semantic::ingest_semantic_document;
use install::{ensure_base_dirs, load_config, save_config};
use store::get_or_open_env;
use store::schema::{AddOutcome, ContextListing, ContextStatus, RecordProcedureInput};

const BOOTSTRAP_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", "CONTEXT.md"];

pub async fn init_context(name: &str) -> Result<ContextStatus> {
    ensure_base_dirs()?;
    seed_default_config()?;

    let ctx_path = context_path(name);
    fs::create_dir_all(blobs_path(&ctx_path))?;
    fs::create_dir_all(index_path(&ctx_path))?;

    let mut manifest = if manifest_path(&ctx_path).exists() {
        Manifest::load(&ctx_path)?
    } else {
        Manifest::empty(name)
    };
    sync_manifest_config(&mut manifest, name)?;
    manifest.save(&ctx_path)?;

    get_or_open_env(&index_path(&ctx_path))?;
    bootstrap_procedural(name).await?;
    context_status(name)
}

pub async fn add_to_context(
    context: &str,
    path: &Path,
    layer: Option<ContentLayer>,
) -> Result<AddOutcome> {
    let ctx_path = open_existing_context(context)?;

    if path.is_dir() {
        let mut total = AddOutcome::default();
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            let entry_path = entry.path();
            if !entry.file_type().is_file() || should_skip_path(entry_path) {
                continue;
            }

            let content = fs::read_to_string(entry_path).with_context(|| {
                format!("failed to read {} as utf-8 text", entry_path.display())
            })?;
            let source_path = display_source_path(entry_path)?;
            let outcome =
                add_content_buffer(&ctx_path, context, &source_path, &content, layer).await?;
            total.chunks_written += outcome.chunks_written;
            total.entities_written += outcome.entities_written;
        }
        return Ok(total);
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read {} as utf-8 text", path.display()))?;
    let source_path = display_source_path(path)?;
    add_content_buffer(&ctx_path, context, &source_path, &content, layer).await
}

pub async fn add_content_to_context(
    context: &str,
    content: &str,
    source: Option<&str>,
    layer: Option<ContentLayer>,
) -> Result<AddOutcome> {
    let ctx_path = open_existing_context(context)?;
    let source_path = source
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("inline/{}.txt", uuid::Uuid::new_v4()));
    add_content_buffer(&ctx_path, context, &source_path, content, layer).await
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

    for entry in manifest.entries {
        if is_virtual_source(&entry.source_path) {
            continue;
        }

        let source_path = PathBuf::from(&entry.source_path);
        if !source_path.exists() {
            continue;
        }

        let current_hash = hash_file(&source_path)?;
        if current_hash != entry.source_hash || entry.status != "indexed" {
            add_to_context(context, &source_path, Some(parse_layer(&entry.layer)?)).await?;
        }
    }

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
            .entries
            .iter()
            .filter_map(|entry| entry.indexed_at)
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
    let manifest = Manifest::load(&ctx_path)?;
    let state = get_or_open_env(&index_path(&ctx_path))?.state();

    let mut indexed_count = 0usize;
    let mut dirty_count = 0usize;
    let mut pending_count = 0usize;

    for entry in &manifest.entries {
        if is_virtual_source(&entry.source_path) {
            indexed_count += 1;
            continue;
        }

        let source_path = PathBuf::from(&entry.source_path);
        if !source_path.exists() {
            pending_count += 1;
            continue;
        }

        let current_hash = hash_file(&source_path)?;
        if current_hash != entry.source_hash {
            dirty_count += 1;
            continue;
        }

        match entry.status.as_str() {
            "pending" => pending_count += 1,
            "dirty" => dirty_count += 1,
            _ => indexed_count += 1,
        }
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
    })
}

async fn bootstrap_procedural(context: &str) -> Result<()> {
    let cwd = std::env::current_dir()?;
    for file_name in BOOTSTRAP_FILES {
        let path = cwd.join(file_name);
        if path.exists() {
            add_to_context(context, &path, Some(ContentLayer::Procedural)).await?;
        }
    }
    Ok(())
}

async fn add_content_buffer(
    ctx_path: &Path,
    context: &str,
    source_path: &str,
    content: &str,
    layer: Option<ContentLayer>,
) -> Result<AddOutcome> {
    let blob = artifact::blob::write_blob(ctx_path, content.as_bytes())?;

    let mut manifest = Manifest::load(ctx_path)?;
    sync_manifest_config(&mut manifest, context)?;
    if let Some(entry) = manifest
        .entries
        .iter()
        .find(|entry| entry.source_path == source_path)
    {
        if entry.source_hash == blob.source_hash && entry.status == "indexed" {
            return Ok(AddOutcome::default());
        }
    }

    let env = get_or_open_env(&index_path(ctx_path))?;
    env.update_state(|state| {
        state.remove_source(source_path);
        Ok(())
    })?;

    let chosen_layer = layer.unwrap_or_else(|| classify_content(Path::new(source_path), content));
    let (summary, chunk_count, entity_count) = match chosen_layer {
        ContentLayer::Semantic => {
            let indexed = ingest_semantic_document(ctx_path, source_path, content).await?;
            (indexed.summary, indexed.chunk_count, indexed.entity_count)
        }
        ContentLayer::Procedural => {
            let indexed =
                ingest_procedural_document(ctx_path, Some(source_path.to_string()), content)
                    .await?;
            (indexed.summary, 0, 0)
        }
    };

    let indexed_at = Utc::now();
    if let Some(entry) = manifest.entry_for_mut(source_path) {
        entry.source_hash = blob.source_hash;
        entry.blob_hash = blob.blob_hash;
        entry.layer = chosen_layer.to_string();
        entry.summary = summary;
        entry.status = String::from("indexed");
        entry.indexed_at = Some(indexed_at);
        entry.chunk_count = chunk_count;
        entry.entity_count = entity_count;
    } else {
        manifest.entries.push(artifact::ManifestEntry {
            id: uuid::Uuid::new_v4().to_string(),
            source_path: source_path.to_string(),
            source_hash: blob.source_hash,
            blob_hash: blob.blob_hash,
            layer: chosen_layer.to_string(),
            summary,
            status: String::from("indexed"),
            indexed_at: Some(indexed_at),
            chunk_count,
            entity_count,
        });
    }
    manifest.save(ctx_path)?;

    Ok(AddOutcome {
        chunks_written: chunk_count,
        entities_written: entity_count,
    })
}

fn seed_default_config() -> Result<()> {
    let path = install::config_path()?;
    if path.exists() {
        return Ok(());
    }

    save_config(&load_config().unwrap_or_default())
}

fn sync_manifest_config(manifest: &mut Manifest, context: &str) -> Result<()> {
    let config = load_config().unwrap_or_default();
    manifest.name = context.to_string();
    manifest.config.splade_enabled = config.splade_enabled;
    manifest.config.extraction_model = config.extraction_model;
    manifest.config.embedding_model = config.embedding_model;
    Ok(())
}

fn open_existing_context(context: &str) -> Result<PathBuf> {
    let ctx_path = context_path(context);
    if !ctx_path.exists() {
        bail!("context {} does not exist", context);
    }
    Ok(ctx_path)
}

fn should_skip_path(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(".git" | ".ctx" | "target" | "node_modules")
        )
    })
}

fn display_source_path(path: &Path) -> Result<String> {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let cwd = std::env::current_dir()?;
    Ok(path
        .strip_prefix(&cwd)
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| path.display().to_string()))
}

fn parse_layer(value: &str) -> Result<ContentLayer> {
    match value {
        "semantic" => Ok(ContentLayer::Semantic),
        "procedural" => Ok(ContentLayer::Procedural),
        _ => bail!("unknown layer {}", value),
    }
}

fn hash_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn is_virtual_source(source_path: &str) -> bool {
    source_path.starts_with("inline/")
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
    }

    #[tokio::test]
    async fn inline_procedural_round_trip() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let tempdir = TempDir::new().expect("tempdir");
        let home_root = TempDir::new().expect("home root");

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
        assert!(results[0].content.contains("deploy to staging"));

        std::env::remove_var("CTX_DISABLE_FASTEMBED");
        std::env::remove_var("CTX_PATH");
        std::env::remove_var("HOME");
    }
}
