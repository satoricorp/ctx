use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use helix_db::helix_engine::storage_core::version_info::VersionInfo;
use helix_db::helix_engine::traversal_core::config::Config as HelixConfig;
use helix_db::helix_engine::traversal_core::HelixGraphEngine;
use helix_db::helix_engine::traversal_core::HelixGraphEngineOpts;

use crate::store::schema::IndexState;

static INDEX_ENVS: OnceLock<Mutex<HashMap<PathBuf, Arc<HelixEnv>>>> = OnceLock::new();

pub struct HelixEnv {
    index_path: PathBuf,
    _engine: HelixGraphEngine,
    state: RwLock<IndexState>,
}

impl std::fmt::Debug for HelixEnv {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HelixEnv")
            .field("index_path", &self.index_path)
            .finish()
    }
}

impl HelixEnv {
    pub fn open(index_path: &Path) -> Result<Self> {
        fs::create_dir_all(index_path)?;
        let engine = open_helix_env(index_path)?;

        Ok(Self {
            index_path: index_path.to_path_buf(),
            _engine: engine,
            state: RwLock::new(load_state(index_path)?),
        })
    }

    pub fn index_path(&self) -> &Path {
        &self.index_path
    }

    pub fn state(&self) -> IndexState {
        self.state.read().expect("state lock poisoned").clone()
    }

    pub fn overwrite(&self, state: IndexState) -> Result<()> {
        *self.state.write().expect("state lock poisoned") = state;
        save_state(
            &self.index_path,
            &self.state.read().expect("state lock poisoned"),
        )
    }

    pub fn update_state<F, T>(&self, operation: F) -> Result<T>
    where
        F: FnOnce(&mut IndexState) -> Result<T>,
    {
        let mut guard = self.state.write().expect("state lock poisoned");
        let output = operation(&mut guard)?;
        save_state(&self.index_path, &guard)?;
        Ok(output)
    }
}

pub fn get_or_open_env(index_path: &Path) -> Result<Arc<HelixEnv>> {
    let registry = INDEX_ENVS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = registry.lock().expect("env registry poisoned");

    if let Some(env) = map.get(index_path) {
        return Ok(Arc::clone(env));
    }

    let env = Arc::new(HelixEnv::open(index_path)?);
    map.insert(index_path.to_owned(), Arc::clone(&env));
    Ok(env)
}

fn open_helix_env(index_path: &Path) -> Result<HelixGraphEngine> {
    let mut config = HelixConfig::default();
    config.mcp = Some(false);
    config.bm25 = Some(true);

    Ok(HelixGraphEngine::new(HelixGraphEngineOpts {
        path: index_path.display().to_string(),
        config,
        version_info: VersionInfo::default(),
    })?)
}

fn state_path(index_path: &Path) -> PathBuf {
    index_path.join("state.json")
}

fn load_state(index_path: &Path) -> Result<IndexState> {
    let path = state_path(index_path);
    if !path.exists() {
        return Ok(IndexState::default());
    }

    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn save_state(index_path: &Path, state: &IndexState) -> Result<()> {
    let path = state_path(index_path);
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(state)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}
