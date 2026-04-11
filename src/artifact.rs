pub mod blob;
pub mod manifest;
pub mod pack;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub use manifest::{Manifest, ManifestConfig, ManifestEntry};

pub fn context_root() -> PathBuf {
    if let Ok(dir) = std::env::var("CTX_PATH") {
        return PathBuf::from(dir);
    }

    dirs::home_dir()
        .expect("no home dir")
        .join(".ctx")
        .join("contexts")
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
