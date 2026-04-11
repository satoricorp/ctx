use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConfig {
    pub id: String,
    pub email: String,
    pub api_key: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub splade_enabled: bool,
    pub extraction_model: String,
    pub embedding_model: String,
    pub models_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<UserConfig>,
    #[serde(default = "default_alpha")]
    pub alpha: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            splade_enabled: false,
            extraction_model: "openai:gpt-4o".into(),
            embedding_model: "fastembed:all-MiniLM-L6-v2".into(),
            models_dir: ctx_home().unwrap_or_else(|_| PathBuf::from(".ctx")).join("models").display().to_string(),
            user: None,
            alpha: default_alpha(),
        }
    }
}

fn default_alpha() -> f32 {
    0.7
}

pub fn ctx_home() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home directory found"))?;
    Ok(home.join(".ctx"))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(ctx_home()?.join("config.json"))
}

pub fn ensure_base_dirs() -> Result<()> {
    let root = ctx_home()?;
    fs::create_dir_all(root.join("contexts"))?;
    fs::create_dir_all(root.join("models"))?;
    Ok(())
}

pub fn load_config() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config::default());
    }

    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub fn save_config(config: &Config) -> Result<()> {
    ensure_base_dirs()?;

    let path = config_path()?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(config)?)?;
    fs::rename(&tmp, &path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

pub fn save_user_config(user: &UserConfig) -> Result<()> {
    let mut config = load_config().unwrap_or_default();
    config.user = Some(user.clone());
    save_config(&config)
}

pub fn auth_header(config: &Config) -> Result<String> {
    let user = config
        .user
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("not authenticated"))?;
    Ok(format!("Bearer {}", user.api_key))
}

pub fn resolve_models_dir(config: &Config) -> PathBuf {
    expand_tilde(&config.models_dir)
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Ok(home) = ctx_home() {
            return home.parent().unwrap_or(Path::new("/")).join(stripped);
        }
    }

    PathBuf::from(path)
}

pub fn ensure_local_setup(interactive: bool) -> Result<Config> {
    ensure_base_dirs()?;
    let config = load_config()?;
    if interactive {
        return Ok(config);
    }

    if !std::env::var("OPENAI_API_KEY").is_ok() && !std::env::var("ANTHROPIC_API_KEY").is_ok() {
        return Ok(config);
    }

    Ok(config)
}

pub fn ensure_model_choice() -> Result<()> {
    bail!("interactive model installation is not implemented yet")
}

