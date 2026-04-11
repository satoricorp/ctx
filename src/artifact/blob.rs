use anyhow::Result;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

use crate::artifact::blobs_path;

#[derive(Debug, Clone)]
pub struct BlobWrite {
    pub source_hash: String,
    pub blob_hash: String,
    pub blob_path: PathBuf,
}

pub fn write_blob(ctx_path: &Path, bytes: &[u8]) -> Result<BlobWrite> {
    let source_hash = hex_digest(bytes);
    let compressed = zstd::stream::encode_all(bytes, 3)?;
    let blob_hash = hex_digest(&compressed);

    let dir = blobs_path(ctx_path);
    fs::create_dir_all(&dir)?;

    let path = dir.join(&blob_hash);
    if !path.exists() {
        fs::write(&path, compressed)?;
    }

    Ok(BlobWrite {
        source_hash: format!("sha256:{source_hash}"),
        blob_hash: format!("sha256:{blob_hash}"),
        blob_path: path,
    })
}

pub fn read_blob(ctx_path: &Path, blob_hash: &str) -> Result<Vec<u8>> {
    let hash = blob_hash.trim_start_matches("sha256:");
    let path = blobs_path(ctx_path).join(hash);
    let bytes = fs::read(path)?;
    Ok(zstd::stream::decode_all(&bytes[..])?)
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

