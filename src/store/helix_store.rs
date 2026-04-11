use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use helix_db::helix_engine::graph_core::config::Config as HelixConfig;
use helix_db::helix_engine::graph_core::graph_core::{HelixGraphEngine, HelixGraphEngineOpts};

use crate::store::schema::IndexState;

static INDEX_ENVS: OnceLock<Mutex<HashMap<PathBuf, Arc<HelixEnv>>>> = OnceLock::new();

#[derive(Debug)]
pub struct HelixEnv {
    index_path: PathBuf,
    state: RwLock<IndexState>,
}

impl HelixEnv {
    pub fn open(index_path: &Path) -> Result<Self> {
        fs::create_dir_all(index_path)?;
        ensure_helix_env(index_path)?;

        Ok(Self {
            index_path: index_path.to_path_buf(),
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
        save_state(&self.index_path, &self.state.read().expect("state lock poisoned"))
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

fn ensure_helix_env(index_path: &Path) -> Result<()> {
    let _engine = HelixGraphEngine::new(HelixGraphEngineOpts {
        path: index_path.display().to_string(),
        config: HelixConfig::default(),
    })?;
    Ok(())
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

