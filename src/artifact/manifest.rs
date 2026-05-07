use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::path::Path;

use crate::artifact::manifest_path;

/// Current manifest schema version (spec §6.3).
pub const MANIFEST_VERSION: &str = "0.2";

/// Rewrites legacy on-disk keys (`ctx_version`, `created`, `entries`) into the current manifest
/// shape so older local artifacts keep loading.
fn migrate_legacy_manifest_map(map: &mut Map<String, Value>) -> Result<()> {
    if !map.contains_key("version") {
        if map.remove("ctx_version").is_some() {
            map.insert(
                "version".to_string(),
                Value::String(MANIFEST_VERSION.into()),
            );
        }
    }
    if !map.contains_key("created_at") {
        if let Some(created) = map.remove("created") {
            map.insert("created_at".to_string(), created.clone());
            map.entry("updated_at".to_string()).or_insert(created);
        }
    }
    if !map.contains_key("sources") {
        if let Some(entries) = map.remove("entries") {
            map.insert(
                "sources".to_string(),
                legacy_entries_to_sources(entries, map)?,
            );
        }
    }
    if !map.contains_key("notes") {
        if let Some(notes) = map.remove("aura") {
            map.insert("notes".to_string(), notes);
        }
    }
    Ok(())
}

fn legacy_entries_to_sources(entries: Value, map: &Map<String, Value>) -> Result<Value> {
    let Value::Array(list) = entries else {
        anyhow::bail!("legacy manifest `entries` must be a JSON array");
    };
    if list.is_empty() {
        return Ok(Value::Array(vec![]));
    }
    let added_at = map
        .get("created_at")
        .or_else(|| map.get("updated_at"))
        .cloned()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "legacy manifest needs `created`/`created_at` to migrate non-empty `entries`"
            )
        })?;
    let mut files = Vec::with_capacity(list.len());
    for item in list {
        let entry = migrate_legacy_entry_to_manifest_entry(item).with_context(|| {
            "legacy manifest `entries` item is not a valid v0.2 file entry (try `ctx doctor` or re-init)"
        })?;
        files.push(serde_json::to_value(entry)?);
    }
    let mut source = Map::new();
    source.insert("root".to_string(), Value::String(".".to_string()));
    source.insert("added_at".to_string(), added_at);
    source.insert("files".to_string(), Value::Array(files));
    Ok(Value::Array(vec![Value::Object(source)]))
}

/// Older builds stored flat `entries` with `source_path` / `source_hash` / `layer` instead of the
/// v0.2 [`ManifestEntry`] field names.
fn migrate_legacy_entry_to_manifest_entry(v: Value) -> Result<ManifestEntry> {
    if let Ok(entry) = serde_json::from_value::<ManifestEntry>(v.clone()) {
        return Ok(entry);
    }
    let Value::Object(mut map) = v else {
        anyhow::bail!("entry must be a JSON object");
    };
    if map.contains_key("source_path") && map.contains_key("source_hash") {
        let source_path = take_json_string(&mut map, "source_path")?;
        let source_hash = take_json_string(&mut map, "source_hash")?;
        let blob_ref = map
            .remove("blob_hash")
            .filter(|v| !v.is_null())
            .map(|v| {
                v.as_str()
                    .map(String::from)
                    .ok_or_else(|| anyhow::anyhow!("blob_hash must be a string"))
            })
            .transpose()?;
        let layer = take_json_string(&mut map, "layer")?;
        let indexed_at: DateTime<Utc> = map
            .remove("indexed_at")
            .map(serde_json::from_value)
            .transpose()?
            .ok_or_else(|| anyhow::anyhow!("missing indexed_at"))?;
        map.remove("hash");
        map.remove("hash_at_index");
        map.remove("path");
        map.remove("type");
        Ok(ManifestEntry {
            path: source_path,
            hash: source_hash.clone(),
            hash_at_index: source_hash,
            indexed_at,
            r#type: layer,
            blob_ref,
            source_path: None,
            extra: map,
        })
    } else {
        serde_json::from_value(Value::Object(map)).with_context(|| "unknown legacy entry shape")
    }
}

fn take_json_string(map: &mut Map<String, Value>, key: &str) -> Result<String> {
    map.remove(key)
        .and_then(|v| v.as_str().map(String::from))
        .ok_or_else(|| anyhow::anyhow!("missing {key}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestConfig {
    // Spec §4.2 fields.
    #[serde(default)]
    pub store_raw_content: bool,
    #[serde(
        default = "default_notes_update_threshold_days",
        alias = "aura_update_threshold_days"
    )]
    pub notes_update_threshold_days: u32,
    #[serde(
        default = "default_notes_update_min_topics",
        alias = "aura_update_min_topics"
    )]
    pub notes_update_min_topics: u32,
    pub embedding_model: String,
    // Retained runtime knobs (not in spec).
    pub splade_enabled: bool,
    pub extraction_model: String,
    /// Unknown JSON keys preserved across round-trips (spec §6.10, §12.4).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn default_notes_update_threshold_days() -> u32 {
    7
}

fn default_notes_update_min_topics() -> u32 {
    3
}

impl Default for ManifestConfig {
    fn default() -> Self {
        Self {
            store_raw_content: false,
            notes_update_threshold_days: default_notes_update_threshold_days(),
            notes_update_min_topics: default_notes_update_min_topics(),
            embedding_model: "openai:text-embedding-3-small".into(),
            splade_enabled: false,
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
pub struct NoteFile {
    pub path: String,
    pub hash: String,
    pub updated_at: DateTime<Utc>,
    /// Unknown JSON keys preserved across round-trips (spec §6.10, §12.4).
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotesRegistry {
    #[serde(default)]
    pub files: Vec<NoteFile>,
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
    #[serde(default, alias = "aura")]
    pub notes: NotesRegistry,
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
            notes: NotesRegistry::default(),
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

        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let mut value: Value =
            serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;
        if let Value::Object(ref mut map) = value {
            migrate_legacy_manifest_map(map)
                .with_context(|| format!("migrate {}", path.display()))?;
        }
        let manifest: Manifest =
            serde_json::from_value(value).with_context(|| format!("parse {}", path.display()))?;

        if manifest.version != MANIFEST_VERSION {
            anyhow::bail!(
                "unsupported manifest version {:?} in {}: expected {:?}",
                manifest.version,
                path.display(),
                MANIFEST_VERSION,
            );
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
        self.sources.last_mut().expect("source just pushed")
    }

    /// Finds the mutable notes entry keyed by `path`, if present.
    pub fn note_entry_for_mut(&mut self, path: &str) -> Option<&mut NoteFile> {
        self.notes.files.iter_mut().find(|entry| entry.path == path)
    }

    /// Inserts or updates a notes registry entry, refreshing `updated_at` only when `hash` changed.
    pub fn upsert_note(&mut self, path: &str, hash: &str) -> &mut NoteFile {
        if let Some(position) = self.notes.files.iter().position(|entry| entry.path == path) {
            let entry = &mut self.notes.files[position];
            if entry.hash != hash {
                entry.hash = hash.to_string();
                entry.updated_at = Utc::now();
            }
            return &mut self.notes.files[position];
        }
        self.notes.files.push(NoteFile {
            path: path.to_string(),
            hash: hash.to_string(),
            updated_at: Utc::now(),
            extra: Map::new(),
        });
        self.notes.files.last_mut().expect("note entry just pushed")
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
                "notes_update_threshold_days": 7,
                "notes_update_min_topics": 3,
                "embedding_model": "openai:text-embedding-3-small",
                "splade_enabled": false,
                "extraction_model": "openai:gpt-5.4-nano"
              }},
              "sources": [],
              "notes": {{ "files": [] }}
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
            "notes_update_threshold_days": 7,
            "notes_update_min_topics": 3,
            "embedding_model": "openai:text-embedding-3-small",
            "splade_enabled": false,
            "extraction_model": "openai:gpt-5.4-nano",
            "custom_knob": 7
          },
          "sources": [],
          "notes": { "files": [] }
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
            "notes_update_threshold_days": 7,
            "notes_update_min_topics": 3,
            "embedding_model": "openai:text-embedding-3-small",
            "splade_enabled": false,
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
          "notes": {
            "policy": "private",
            "files": [
              {
                "path": "notes/index.md",
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
            manifest.notes.extra.get("policy"),
            Some(&Value::String("private".into()))
        );
        assert_eq!(
            manifest.notes.files[0].extra.get("owner"),
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
    fn load_rejects_version_1() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        write_manifest(dir.path(), &minimal_manifest_json("1"));
        let err = Manifest::load(dir.path()).expect_err("must fail");
        let message = format!("{err:#}");
        assert!(message.contains("\"1\""), "got {message}");
        assert!(message.contains("\"0.2\""), "got {message}");
    }

    #[test]
    fn load_migrates_legacy_ctx_version_and_entries() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let json = r#"{
          "ctx_version": "1.0",
          "name": "homes",
          "created": "2026-04-15T20:34:25.331072Z",
          "config": {
            "splade_enabled": false,
            "extraction_model": "openai:gpt-5.4-nano",
            "embedding_model": "openai:text-embedding-3-small"
          },
          "entries": []
        }"#;
        write_manifest(dir.path(), json);
        let manifest = Manifest::load(dir.path()).expect("load legacy");
        assert_eq!(manifest.version, "0.2");
        assert_eq!(manifest.name, "homes");
        assert!(manifest.sources.is_empty());
        assert!(!manifest.config.store_raw_content);
    }

    #[test]
    fn load_migrates_legacy_flat_entries_with_source_path() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let json = r#"{
          "ctx_version": "1.0",
          "name": "demo",
          "created": "2026-04-13T19:20:19.010418Z",
          "config": {
            "splade_enabled": false,
            "extraction_model": "openai:gpt-5.4-nano",
            "embedding_model": "openai:text-embedding-3-small"
          },
          "entries": [
            {
              "id": "64eff7dd-4b9e-4687-92a1-f94e7f942d9f",
              "source_path": "longmemeval/e47becba/sharegpt_yywfIrx_0.md",
              "source_hash": "sha256:3c37e6cf38f1c18f1b3abb8d0f18e1354540d55f24bdeaf0f9cc4e1f619a6387",
              "blob_hash": "sha256:4189d6d4b970816c418113ff9636cf9356fb4497bbf27b2fdf1daf35d06c388e",
              "layer": "semantic",
              "summary": "summary",
              "status": "indexed",
              "indexed_at": "2026-04-13T19:21:43.805924Z",
              "chunk_count": 16,
              "entity_count": 101
            }
          ]
        }"#;
        write_manifest(dir.path(), json);
        let manifest = Manifest::load(dir.path()).expect("load legacy flat entries");
        assert_eq!(manifest.sources.len(), 1);
        assert_eq!(manifest.sources[0].root, ".");
        assert_eq!(manifest.sources[0].files.len(), 1);
        let f = &manifest.sources[0].files[0];
        assert_eq!(f.path, "longmemeval/e47becba/sharegpt_yywfIrx_0.md");
        assert_eq!(f.r#type, "semantic");
        assert_eq!(
            f.blob_ref.as_deref(),
            Some("sha256:4189d6d4b970816c418113ff9636cf9356fb4497bbf27b2fdf1daf35d06c388e")
        );
        assert!(f.extra.contains_key("id"));
    }
}
