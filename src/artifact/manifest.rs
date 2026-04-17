use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::path::Path;

use crate::artifact::manifest_path;

/// Current manifest schema version (spec §6.3).
pub const MANIFEST_VERSION: &str = "0.2";
/// Legacy v1 label used before the spec v0.2 alignment. Loaded but rewritten on save.
const LEGACY_MANIFEST_VERSION: &str = "1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestConfig {
    // Spec §4.2 fields.
    pub store_raw_content: bool,
    #[serde(default = "default_aura_update_threshold_days")]
    pub aura_update_threshold_days: u32,
    #[serde(default = "default_aura_update_min_topics")]
    pub aura_update_min_topics: u32,
    pub embedding_model: String,
    // Retained runtime knobs (not in spec).
    pub splade_enabled: bool,
    pub extraction_model: String,
    /// Unknown JSON keys preserved across round-trips (spec §6.10, §12.4).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn default_aura_update_threshold_days() -> u32 {
    7
}

fn default_aura_update_min_topics() -> u32 {
    3
}

impl Default for ManifestConfig {
    fn default() -> Self {
        Self {
            store_raw_content: false,
            aura_update_threshold_days: default_aura_update_threshold_days(),
            aura_update_min_topics: default_aura_update_min_topics(),
            embedding_model: "fastembed:all-MiniLM-L6-v2".into(),
            splade_enabled: true,
            extraction_model: "openai:gpt-5.4-nano".into(),
            extra: Map::new(),
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
    /// Real file path when `path` is a virtual unit (e.g. a PDF page or spreadsheet sheet).
    /// `None` when the entry represents the entire source file (default for plain-text sources).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    /// Unknown JSON keys preserved across round-trips (spec §6.10, §12.4).
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
}

impl ManifestEntry {
    /// Real on-disk path to hash for drift detection. Equals `path` unless the entry
    /// was produced by a multi-unit decoder that set `source_path` explicitly.
    pub fn effective_source_path(&self) -> &str {
        self.source_path.as_deref().unwrap_or(&self.path)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEntry {
    pub root: String,
    pub added_at: DateTime<Utc>,
    pub files: Vec<ManifestEntry>,
    /// Unknown JSON keys preserved across round-trips (spec §6.10, §12.4).
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuraFile {
    pub path: String,
    pub hash: String,
    pub updated_at: DateTime<Utc>,
    /// Unknown JSON keys preserved across round-trips (spec §6.10, §12.4).
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuraRegistry {
    #[serde(default)]
    pub files: Vec<AuraFile>,
    /// Unknown JSON keys preserved across round-trips (spec §6.10, §12.4).
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
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
    /// Unknown JSON keys preserved across round-trips (spec §6.10, §12.4).
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
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
            extra: Map::new(),
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

        let mut manifest: Manifest = serde_json::from_slice(
            &fs::read(&path).with_context(|| format!("read {}", path.display()))?,
        )
        .with_context(|| format!("parse {}", path.display()))?;

        if manifest.version == LEGACY_MANIFEST_VERSION {
            manifest.version = MANIFEST_VERSION.to_string();
        } else if manifest.version != MANIFEST_VERSION {
            anyhow::bail!(
                "unsupported manifest version {:?} in {}: expected {:?}",
                manifest.version,
                path.display(),
                MANIFEST_VERSION,
            );
        }

        if let Some(v) = manifest.config.extra.remove("promotion_threshold_days") {
            if let Some(n) = v.as_u64() {
                manifest.config.aura_update_threshold_days = n as u32;
            }
        }
        if let Some(v) = manifest.config.extra.remove("promotion_min_occurrences") {
            if let Some(n) = v.as_u64() {
                manifest.config.aura_update_min_topics = n as u32;
            }
        }

        Ok(manifest)
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
            extra: Map::new(),
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
            extra: Map::new(),
        });
        self.aura.files.last_mut().expect("aura entry just pushed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_source_path_defaults_to_path() {
        let entry = ManifestEntry {
            path: "docs/notes.md".into(),
            hash: "sha256:0".into(),
            hash_at_index: "sha256:0".into(),
            indexed_at: Utc::now(),
            r#type: "semantic".into(),
            blob_ref: None,
            source_path: None,
            extra: Map::new(),
        };
        assert_eq!(entry.effective_source_path(), "docs/notes.md");
    }

    #[test]
    fn effective_source_path_honors_virtual_entries() {
        let entry = ManifestEntry {
            path: "docs/report.pdf/page-3.txt".into(),
            hash: "sha256:0".into(),
            hash_at_index: "sha256:0".into(),
            indexed_at: Utc::now(),
            r#type: "semantic".into(),
            blob_ref: None,
            source_path: Some("docs/report.pdf".into()),
            extra: Map::new(),
        };
        assert_eq!(entry.effective_source_path(), "docs/report.pdf");
    }

    #[test]
    fn source_path_is_omitted_when_none() {
        let entry = ManifestEntry {
            path: "a.md".into(),
            hash: "sha256:0".into(),
            hash_at_index: "sha256:0".into(),
            indexed_at: Utc::now(),
            r#type: "semantic".into(),
            blob_ref: None,
            source_path: None,
            extra: Map::new(),
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(!json.contains("source_path"), "got {json}");
    }

    #[test]
    fn source_path_round_trips_when_set() {
        let entry = ManifestEntry {
            path: "a.pdf/page-1.txt".into(),
            hash: "sha256:0".into(),
            hash_at_index: "sha256:0".into(),
            indexed_at: Utc::now(),
            r#type: "semantic".into(),
            blob_ref: None,
            source_path: Some("a.pdf".into()),
            extra: Map::new(),
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: ManifestEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.source_path.as_deref(), Some("a.pdf"));
    }

    fn write_manifest(dir: &Path, json: &str) {
        fs::create_dir_all(dir).expect("mkdir");
        fs::write(manifest_path(dir), json).expect("write manifest");
    }

    fn minimal_manifest_json(version: &str) -> String {
        format!(
            r#"{{
              "version": "{version}",
              "name": "demo",
              "created_at": "2026-04-16T00:00:00Z",
              "updated_at": "2026-04-16T00:00:00Z",
              "config": {{
                "store_raw_content": false,
                "aura_update_threshold_days": 7,
                "aura_update_min_topics": 3,
                "embedding_model": "fastembed:all-MiniLM-L6-v2",
                "splade_enabled": true,
                "extraction_model": "openai:gpt-5.4-nano"
              }},
              "sources": [],
              "aura": {{ "files": [] }}
            }}"#
        )
    }

    #[test]
    fn load_accepts_0_2_version() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        write_manifest(dir.path(), &minimal_manifest_json("0.2"));
        let manifest = Manifest::load(dir.path()).expect("load");
        assert_eq!(manifest.version, "0.2");
    }

    #[test]
    fn load_migrates_legacy_1_version() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        write_manifest(dir.path(), &minimal_manifest_json("1"));
        let manifest = Manifest::load(dir.path()).expect("load");
        assert_eq!(manifest.version, "0.2");
    }

    #[test]
    fn load_rejects_unknown_version() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        write_manifest(dir.path(), &minimal_manifest_json("0.3"));
        let err = Manifest::load(dir.path()).expect_err("must fail");
        let message = format!("{err:#}");
        assert!(message.contains("\"0.3\""), "got {message}");
        assert!(message.contains("\"0.2\""), "got {message}");
    }

    #[test]
    fn unknown_top_level_fields_round_trip() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let json = r#"{
          "version": "0.2",
          "name": "demo",
          "encryption_scheme": "aes-gcm",
          "created_at": "2026-04-16T00:00:00Z",
          "updated_at": "2026-04-16T00:00:00Z",
          "config": {
            "store_raw_content": false,
            "aura_update_threshold_days": 7,
            "aura_update_min_topics": 3,
            "embedding_model": "fastembed:all-MiniLM-L6-v2",
            "splade_enabled": true,
            "extraction_model": "openai:gpt-5.4-nano",
            "custom_knob": 7
          },
          "sources": [],
          "aura": { "files": [] }
        }"#;
        write_manifest(dir.path(), json);

        let mut manifest = Manifest::load(dir.path()).expect("load");
        assert_eq!(
            manifest.extra.get("encryption_scheme"),
            Some(&Value::String("aes-gcm".into()))
        );
        assert_eq!(
            manifest.config.extra.get("custom_knob"),
            Some(&Value::Number(7.into()))
        );

        manifest.save(dir.path()).expect("save");
        let raw = fs::read_to_string(manifest_path(dir.path())).expect("read");
        assert!(raw.contains("\"encryption_scheme\""), "got {raw}");
        assert!(raw.contains("\"custom_knob\""), "got {raw}");
    }

    #[test]
    fn unknown_fields_round_trip_on_nested_structs() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let json = r#"{
          "version": "0.2",
          "name": "demo",
          "created_at": "2026-04-16T00:00:00Z",
          "updated_at": "2026-04-16T00:00:00Z",
          "config": {
            "store_raw_content": false,
            "aura_update_threshold_days": 7,
            "aura_update_min_topics": 3,
            "embedding_model": "fastembed:all-MiniLM-L6-v2",
            "splade_enabled": true,
            "extraction_model": "openai:gpt-5.4-nano"
          },
          "sources": [
            {
              "root": "/tmp/src",
              "added_at": "2026-04-16T00:00:00Z",
              "provenance": "upstream",
              "files": [
                {
                  "path": "a.md",
                  "hash": "sha256:0",
                  "hash_at_index": "sha256:0",
                  "indexed_at": "2026-04-16T00:00:00Z",
                  "type": "semantic",
                  "note": "pinned"
                }
              ]
            }
          ],
          "aura": {
            "policy": "private",
            "files": [
              {
                "path": "aura/index.md",
                "hash": "sha256:0",
                "updated_at": "2026-04-16T00:00:00Z",
                "owner": "alice"
              }
            ]
          }
        }"#;
        write_manifest(dir.path(), json);

        let mut manifest = Manifest::load(dir.path()).expect("load");
        assert_eq!(
            manifest.sources[0].extra.get("provenance"),
            Some(&Value::String("upstream".into()))
        );
        assert_eq!(
            manifest.sources[0].files[0].extra.get("note"),
            Some(&Value::String("pinned".into()))
        );
        assert_eq!(
            manifest.aura.extra.get("policy"),
            Some(&Value::String("private".into()))
        );
        assert_eq!(
            manifest.aura.files[0].extra.get("owner"),
            Some(&Value::String("alice".into()))
        );

        manifest.save(dir.path()).expect("save");
        let raw = fs::read_to_string(manifest_path(dir.path())).expect("read");
        assert!(raw.contains("\"provenance\""), "got {raw}");
        assert!(raw.contains("\"note\""), "got {raw}");
        assert!(raw.contains("\"policy\""), "got {raw}");
        assert!(raw.contains("\"owner\""), "got {raw}");
    }

    #[test]
    fn load_migrates_legacy_promotion_config_fields() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let json = r#"{
          "version": "0.2",
          "name": "demo",
          "created_at": "2026-04-16T00:00:00Z",
          "updated_at": "2026-04-16T00:00:00Z",
          "config": {
            "store_raw_content": false,
            "promotion_threshold_days": 14,
            "promotion_min_occurrences": 5,
            "embedding_model": "fastembed:all-MiniLM-L6-v2",
            "splade_enabled": true,
            "extraction_model": "openai:gpt-5.4-nano"
          },
          "sources": [],
          "aura": { "files": [] }
        }"#;
        write_manifest(dir.path(), json);

        let mut manifest = Manifest::load(dir.path()).expect("load");
        assert_eq!(manifest.config.aura_update_threshold_days, 14);
        assert_eq!(manifest.config.aura_update_min_topics, 5);
        assert!(
            !manifest.config.extra.contains_key("promotion_threshold_days"),
            "extra still has promotion_threshold_days"
        );
        assert!(
            !manifest.config.extra.contains_key("promotion_min_occurrences"),
            "extra still has promotion_min_occurrences"
        );

        manifest.save(dir.path()).expect("save");
        let raw = fs::read_to_string(manifest_path(dir.path())).expect("read");
        assert!(raw.contains("\"aura_update_threshold_days\""), "got {raw}");
        assert!(raw.contains("\"aura_update_min_topics\""), "got {raw}");
        assert!(!raw.contains("\"promotion_threshold_days\""), "got {raw}");
        assert!(!raw.contains("\"promotion_min_occurrences\""), "got {raw}");
    }
}
