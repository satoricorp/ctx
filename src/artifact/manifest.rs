use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::artifact::manifest_path;

/// Current manifest schema version (spec §4.1).
pub const MANIFEST_VERSION: &str = "1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestConfig {
    // Spec §4.2 fields.
    pub store_raw_content: bool,
    pub promotion_threshold_days: u32,
    pub promotion_min_occurrences: u32,
    pub embedding_model: String,
    // Retained runtime knobs (not in spec).
    pub splade_enabled: bool,
    pub extraction_model: String,
}

impl Default for ManifestConfig {
    fn default() -> Self {
        Self {
            store_raw_content: false,
            promotion_threshold_days: 7,
            promotion_min_occurrences: 3,
            embedding_model: "fastembed:all-MiniLM-L6-v2".into(),
            splade_enabled: true,
            extraction_model: "openai:gpt-5.4-nano".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub path: String,
    pub hash: String,
    pub hash_at_index: String,
    pub indexed_at: DateTime<Utc>,
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEntry {
    pub root: String,
    pub added_at: DateTime<Utc>,
    pub files: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuraFile {
    pub path: String,
    pub hash: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuraRegistry {
    #[serde(default)]
    pub files: Vec<AuraFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub config: ManifestConfig,
    #[serde(default)]
    pub sources: Vec<SourceEntry>,
    #[serde(default)]
    pub aura: AuraRegistry,
}

impl Manifest {
    pub fn empty(name: &str) -> Self {
        let now = Utc::now();
        Self {
            version: MANIFEST_VERSION.into(),
            name: name.into(),
            created_at: now,
            updated_at: now,
            config: ManifestConfig::default(),
            sources: Vec::new(),
            aura: AuraRegistry::default(),
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

        serde_json::from_slice(
            &fs::read(&path).with_context(|| format!("read {}", path.display()))?,
        )
        .with_context(|| format!("parse {}", path.display()))
    }

    /// Persists the manifest, bumping `updated_at` to now.
    pub fn save(&mut self, ctx_path: &Path) -> Result<()> {
        self.updated_at = Utc::now();
        let path = manifest_path(ctx_path);
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    /// Finds the mutable `SourceEntry` keyed by `root`, if present.
    pub fn source_for_mut(&mut self, root: &str) -> Option<&mut SourceEntry> {
        self.sources.iter_mut().find(|entry| entry.root == root)
    }

    /// Finds the mutable `ManifestEntry` at `root`/`path`, if present.
    pub fn entry_for_mut(&mut self, root: &str, path: &str) -> Option<&mut ManifestEntry> {
        self.source_for_mut(root)
            .and_then(|source| source.files.iter_mut().find(|entry| entry.path == path))
    }

    /// Inserts or returns the existing `SourceEntry` keyed by `root`.
    pub fn upsert_source(&mut self, root: &str) -> &mut SourceEntry {
        if let Some(position) = self.sources.iter().position(|entry| entry.root == root) {
            return &mut self.sources[position];
        }
        self.sources.push(SourceEntry {
            root: root.to_string(),
            added_at: Utc::now(),
            files: Vec::new(),
        });
        self.sources
            .last_mut()
            .expect("source just pushed")
    }

    /// Finds the mutable `AuraFile` keyed by `path`, if present.
    pub fn aura_entry_for_mut(&mut self, path: &str) -> Option<&mut AuraFile> {
        self.aura.files.iter_mut().find(|entry| entry.path == path)
    }

    /// Inserts or updates an aura registry entry, refreshing `updated_at` only when `hash` changed.
    pub fn upsert_aura(&mut self, path: &str, hash: &str) -> &mut AuraFile {
        if let Some(position) = self.aura.files.iter().position(|entry| entry.path == path) {
            let entry = &mut self.aura.files[position];
            if entry.hash != hash {
                entry.hash = hash.to_string();
                entry.updated_at = Utc::now();
            }
            return &mut self.aura.files[position];
        }
        self.aura.files.push(AuraFile {
            path: path.to_string(),
            hash: hash.to_string(),
            updated_at: Utc::now(),
        });
        self.aura.files.last_mut().expect("aura entry just pushed")
    }
}
