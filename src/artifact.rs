pub mod blob;
pub mod manifest;
pub mod pack;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub use manifest::{Manifest, ManifestConfig, ManifestEntry};

/// Base directory for context stores (`CTX_PATH` or `~/.ctx/contexts`).
pub fn context_root_base() -> PathBuf {
    if let Ok(dir) = std::env::var("CTX_PATH") {
        return PathBuf::from(dir);
    }

    dirs::home_dir()
        .expect("no home dir")
        .join(".ctx")
        .join("contexts")
}

/// Effective contexts directory. Per spec §11 this is always the base; **CTX_IMAGE** scoping
/// happens at the artifact-name level in [`context_path`], not via an `images/` subdirectory.
pub fn context_root() -> PathBuf {
    context_root_base()
}

fn sanitize_image_segment(tag: &str) -> String {
    tag.chars()
        .map(|c| {
            if c == '/' || c == '\\' || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect()
}

/// Resolve the artifact directory for a named context. When **CTX_IMAGE** is set (and non-empty),
/// the image tag replaces `name`, producing `…/contexts/<tag>.ctx`.
pub fn context_path(name: &str) -> PathBuf {
    let effective = match std::env::var("CTX_IMAGE") {
        Ok(tag) if !tag.trim().is_empty() => sanitize_image_segment(tag.trim()),
        _ => name.to_string(),
    };
    context_root().join(format!("{}.ctx", effective))
}

pub fn index_path(ctx_path: &Path) -> PathBuf {
    ctx_path.join("index")
}

pub fn manifest_path(ctx_path: &Path) -> PathBuf {
    ctx_path.join("manifest.json")
}

pub fn blobs_path(ctx_path: &Path) -> PathBuf {
    ctx_path.join("blobs").join("sha256")
}

pub fn aura_path(ctx_path: &Path) -> PathBuf {
    ctx_path.join("aura")
}

pub fn infer_context_name() -> Result<String> {
    std::env::current_dir()?
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .context("could not infer context name from current directory")
}
