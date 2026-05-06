use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_EMBEDDING_MODEL: &str = "openai:text-embedding-3-small";
const DEFAULT_EXTRACTION_MODEL: &str = "openai:gpt-5.4-nano";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConfig {
    pub id: String,
    pub email: String,
    pub api_key: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub splade_enabled: bool,
    #[serde(default = "default_extraction_model")]
    pub extraction_model: String,
    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<UserConfig>,
    #[serde(default = "default_alpha")]
    pub alpha: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_context: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            splade_enabled: false,
            extraction_model: default_extraction_model(),
            embedding_model: default_embedding_model(),
            user: None,
            alpha: default_alpha(),
            default_context: None,
        }
    }
}

fn default_alpha() -> f32 {
    0.7
}

fn default_embedding_model() -> String {
    DEFAULT_EMBEDDING_MODEL.into()
}

fn default_extraction_model() -> String {
    DEFAULT_EXTRACTION_MODEL.into()
}

pub fn ctx_home() -> Result<PathBuf> {
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return Ok(PathBuf::from(home).join(".ctx"));
        }
    }

    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home directory found"))?;
    Ok(home.join(".ctx"))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(ctx_home()?.join("config.json"))
}

pub fn ensure_base_dirs() -> Result<()> {
    let root = ctx_home()?;
    ensure_secure_dir(&root)?;
    fs::create_dir_all(root.join("contexts"))?;
    Ok(())
}

pub fn load_config() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config::default());
    }

    let mut config: Config = serde_json::from_slice(&fs::read(path)?)?;
    normalize_config(&mut config);
    Ok(config)
}

pub fn save_config(config: &Config) -> Result<()> {
    ensure_base_dirs()?;

    let path = config_path()?;
    write_config_atomically(&path, &serde_json::to_vec_pretty(config)?)
}

pub fn save_user_config(user: &UserConfig) -> Result<()> {
    let mut config = load_config().unwrap_or_default();
    config.user = Some(user.clone());
    save_config(&config)
}

pub fn default_context_selection() -> Option<String> {
    let s = load_config().ok()?.default_context?;
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

pub fn set_default_context(context: &str) -> Result<()> {
    let trimmed = context.trim();
    if trimmed.is_empty() {
        bail!("default context cannot be empty");
    }
    let mut config = load_config()?;
    config.default_context = Some(trimmed.to_string());
    save_config(&config)
}

pub fn set_default_context_if_unset(context: &str) -> Result<()> {
    let trimmed = context.trim();
    if trimmed.is_empty() {
        bail!("context name cannot be empty");
    }
    let mut config = load_config()?;
    let unset = config
        .default_context
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none();
    if unset {
        config.default_context = Some(trimmed.to_string());
        save_config(&config)?;
    }
    Ok(())
}

pub fn effective_alpha(config: &Config) -> f32 {
    std::env::var("CTX_ALPHA")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|alpha| alpha.is_finite())
        .map(|alpha| alpha.clamp(0.0, 1.0))
        .unwrap_or(config.alpha)
}

pub fn auth_header(config: &Config) -> Result<String> {
    let user = config
        .user
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("not authenticated"))?;
    Ok(format!("Bearer {}", user.api_key))
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Ok(home) = ctx_home() {
            return home.parent().unwrap_or(Path::new("/")).join(stripped);
        }
    }

    PathBuf::from(path)
}

pub fn ensure_local_setup() -> Result<Config> {
    ensure_base_dirs()?;

    if !openai_api_key_present() {
        bail!("OPENAI_API_KEY is required. Export it in your shell before running `ctx init`.");
    }

    let path = config_path()?;
    let had_config = path.exists();
    let mut config = load_config()?;
    let mut changed = !had_config;

    if config.extraction_model != DEFAULT_EXTRACTION_MODEL {
        config.extraction_model = DEFAULT_EXTRACTION_MODEL.into();
        changed = true;
    }
    if config.embedding_model != DEFAULT_EMBEDDING_MODEL {
        config.embedding_model = DEFAULT_EMBEDDING_MODEL.into();
        changed = true;
    }
    if config.splade_enabled {
        config.splade_enabled = false;
        changed = true;
    }
    if !(0.0..=1.0).contains(&config.alpha) {
        config.alpha = default_alpha();
        changed = true;
    }

    if changed {
        save_config(&config)?;
    }

    Ok(config)
}

fn normalize_config(config: &mut Config) {
    if !config.extraction_model.starts_with("openai:") {
        config.extraction_model = DEFAULT_EXTRACTION_MODEL.into();
    }
    if !config.embedding_model.starts_with("openai:") {
        config.embedding_model = DEFAULT_EMBEDDING_MODEL.into();
    }
    config.splade_enabled = false;
    if !(0.0..=1.0).contains(&config.alpha) {
        config.alpha = default_alpha();
    }
}

fn openai_api_key_present() -> bool {
    std::env::var("OPENAI_API_KEY")
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn ensure_secure_dir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("failed to secure {}", dir.display()))?;
    }

    Ok(())
}

fn write_config_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("config path has no parent"))?;
    fs::create_dir_all(parent)?;

    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_uses_home_directory() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let expected = dirs::home_dir().expect("home directory").join(".ctx/test");
        assert_eq!(expand_tilde("~/.ctx/test"), expected);
    }

    #[test]
    fn ctx_home_prefers_home_env() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", "/tmp/ctx-home-test");
        assert_eq!(
            ctx_home().expect("ctx home"),
            PathBuf::from("/tmp/ctx-home-test/.ctx")
        );
        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }
    }

    #[test]
    fn normalize_config_rewrites_non_openai_models() {
        let mut config = Config {
            splade_enabled: true,
            extraction_model: "custom:extraction".into(),
            embedding_model: "custom:embedding".into(),
            ..Config::default()
        };
        normalize_config(&mut config);
        assert_eq!(config.extraction_model, DEFAULT_EXTRACTION_MODEL);
        assert_eq!(config.embedding_model, DEFAULT_EMBEDDING_MODEL);
        assert!(!config.splade_enabled);
    }

    #[test]
    fn ensure_local_setup_requires_openai_key() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let original = std::env::var("OPENAI_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");
        let error = ensure_local_setup().expect_err("missing api key");
        assert!(error
            .to_string()
            .contains("OPENAI_API_KEY is required"));
        if let Some(value) = original {
            std::env::set_var("OPENAI_API_KEY", value);
        }
    }

    #[test]
    fn effective_alpha_prefers_env_and_clamps() {
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("test lock poisoned");
        let original = std::env::var("CTX_ALPHA").ok();
        let config = Config {
            alpha: 0.7,
            ..Config::default()
        };

        std::env::remove_var("CTX_ALPHA");
        assert_eq!(effective_alpha(&config), 0.7);

        std::env::set_var("CTX_ALPHA", "0.9");
        assert_eq!(effective_alpha(&config), 0.9);

        std::env::set_var("CTX_ALPHA", "2.5");
        assert_eq!(effective_alpha(&config), 1.0);

        std::env::set_var("CTX_ALPHA", "-1.0");
        assert_eq!(effective_alpha(&config), 0.0);

        std::env::set_var("CTX_ALPHA", "not-a-number");
        assert_eq!(effective_alpha(&config), 0.7);

        if let Some(value) = original {
            std::env::set_var("CTX_ALPHA", value);
        } else {
            std::env::remove_var("CTX_ALPHA");
        }
    }
}
