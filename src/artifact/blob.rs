use anyhow::Result;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::artifact::blobs_path;

#[derive(Debug, Clone)]
pub struct BlobWrite {
    pub source_hash: String,
    pub blob_hash: String,
    pub blob_path: PathBuf,
}

/// Errors raised by [`read_verified`] when on-disk bytes fail to match the manifest
/// (spec §13.1, §13.5). Callers MUST treat these as integrity failures.
#[derive(Debug, Error)]
pub enum IntegrityError {
    #[error("blob {0} missing from store")]
    Missing(String),
    #[error("blob digest mismatch: expected {expected}, got {actual}")]
    BlobDigest { expected: String, actual: String },
    #[error("content digest mismatch: expected {expected}, got {actual}")]
    ContentDigest { expected: String, actual: String },
    #[error("io error reading blob: {0}")]
    Io(#[from] std::io::Error),
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

/// Reads a blob by `blob_ref`, verifying that (a) the on-disk compressed bytes hash to
/// `blob_ref` and (b) the decompressed content hashes to `expected_content_hash`.
/// Spec §13.1 / §13.5: integrity failures MUST be surfaced rather than silently trusted.
pub fn read_verified(
    ctx_path: &Path,
    blob_ref: &str,
    expected_content_hash: &str,
) -> std::result::Result<Vec<u8>, IntegrityError> {
    let hash = blob_ref.trim_start_matches("sha256:");
    let path = blobs_path(ctx_path).join(hash);
    if !path.exists() {
        return Err(IntegrityError::Missing(blob_ref.to_string()));
    }

    let compressed = fs::read(&path)?;
    let actual_blob_hash = format!("sha256:{}", hex_digest(&compressed));
    if actual_blob_hash != blob_ref {
        return Err(IntegrityError::BlobDigest {
            expected: blob_ref.to_string(),
            actual: actual_blob_hash,
        });
    }

    let bytes = zstd::stream::decode_all(&compressed[..])
        .map_err(|err| IntegrityError::Io(std::io::Error::other(err)))?;
    let actual_content_hash = format!("sha256:{}", hex_digest(&bytes));
    if actual_content_hash != expected_content_hash {
        return Err(IntegrityError::ContentDigest {
            expected: expected_content_hash.to_string(),
            actual: actual_content_hash,
        });
    }

    Ok(bytes)
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn read_verified_round_trips() {
        let dir = TempDir::new().expect("tempdir");
        let bytes = b"hello blob world";
        let written = write_blob(dir.path(), bytes).expect("write");

        let got = read_verified(dir.path(), &written.blob_hash, &written.source_hash)
            .expect("verify");
        assert_eq!(got, bytes);
    }

    #[test]
    fn read_verified_detects_tampered_blob() {
        let dir = TempDir::new().expect("tempdir");
        let bytes = b"pristine content";
        let written = write_blob(dir.path(), bytes).expect("write");

        fs::write(&written.blob_path, b"corrupted bytes").expect("tamper");

        let err = read_verified(dir.path(), &written.blob_hash, &written.source_hash)
            .expect_err("must fail");
        assert!(matches!(err, IntegrityError::BlobDigest { .. }), "got {err:?}");
    }

    #[test]
    fn read_verified_detects_missing_blob() {
        let dir = TempDir::new().expect("tempdir");
        let err = read_verified(
            dir.path(),
            "sha256:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .expect_err("must fail");
        assert!(matches!(err, IntegrityError::Missing(_)), "got {err:?}");
    }
}
