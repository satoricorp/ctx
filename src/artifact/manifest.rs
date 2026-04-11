use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::artifact::manifest_path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestConfig {
    pub splade_enabled: bool,
    pub extraction_model: String,
    pub embedding_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub id: String,
    pub source_path: String,
    pub source_hash: String,
    pub blob_hash: String,
    pub layer: String,
    pub summary: String,
    pub status: String,
    pub indexed_at: Option<DateTime<Utc>>,
    pub chunk_count: usize,
    pub entity_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub ctx_version: String,
    pub name: String,
    pub created: DateTime<Utc>,
    pub config: ManifestConfig,
    pub entries: Vec<ManifestEntry>,
}

impl Manifest {
    pub fn empty(name: &str) -> Self {
        Self {
            ctx_version: "1.0".into(),
            name: name.into(),
            created: Utc::now(),
            config: ManifestConfig {
                splade_enabled: true,
                extraction_model: "openai:gpt-4o".into(),
                embedding_model: "fastembed:all-MiniLM-L6-v2".into(),
            },
            entries: Vec::new(),
        }
    }

    pub fn load(ctx_path: &Path) -> Result<Self> {
        let path = manifest_path(ctx_path);
        if !path.exists() {
            return Ok(Self::empty(
                ctx_path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("default"),
            ));
        }

        serde_json::from_slice(&fs::read(&path).with_context(|| format!("read {}", path.display()))?)
            .with_context(|| format!("parse {}", path.display()))
    }

    pub fn save(&self, ctx_path: &Path) -> Result<()> {
        let path = manifest_path(ctx_path);
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn entry_for_mut(&mut self, source_path: &str) -> Option<&mut ManifestEntry> {
        self.entries.iter_mut().find(|entry| entry.source_path == source_path)
    }
}

