pub mod blob;
pub mod manifest;
pub mod pack;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub use manifest::{Manifest, ManifestConfig, ManifestEntry};

/// Base directory for context stores (`CTX_PATH` or `~/.ctx/contexts`), before **CTX_IMAGE** scoping.
pub fn context_root_base() -> PathBuf {
    if let Ok(dir) = std::env::var("CTX_PATH") {
        return PathBuf::from(dir);
    }

    dirs::home_dir()
        .expect("no home dir")
        .join(".ctx")
        .join("contexts")
}

/// Effective contexts directory: **`context_root_base()`**, or **`…/images/<CTX_IMAGE>/`** when set.
pub fn context_root() -> PathBuf {
    let base = context_root_base();
    if let Ok(image) = std::env::var("CTX_IMAGE") {
        let tag = image.trim();
        if !tag.is_empty() {
            return base.join("images").join(sanitize_image_segment(tag));
        }
    }
    base
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

pub fn context_path(name: &str) -> PathBuf {
    context_root().join(format!("{}.ctx", name))
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

pub fn infer_context_name() -> Result<String> {
    std::env::current_dir()?
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .context("could not infer context name from current directory")
}
