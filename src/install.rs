use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use sysinfo::System;

use crate::models::embeddings::{install_required_fastembed_assets, install_splade_asset};
use crate::models::llm::ensure_local_extraction_model;

const DEFAULT_EMBEDDING_MODEL: &str = "fastembed:all-MiniLM-L6-v2";
const DEFAULT_EXTRACTION_MODEL: &str = "openai:gpt-4o";
const DEFAULT_ANTHROPIC_MODEL: &str = "anthropic:claude-sonnet-4-6";
const DEFAULT_MODELS_DIR: &str = "~/.ctx/models";
const LOCAL_EXTRACTION_SMALL: &str = "gemma4-e4b";
const LOCAL_EXTRACTION_LARGE: &str = "gemma4-26b-a4b";
const UNCONFIGURED_EXTRACTION_MODEL: &str = "unconfigured";

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
    #[serde(default = "default_models_dir")]
    pub models_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<UserConfig>,
    #[serde(default = "default_alpha")]
    pub alpha: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApiKeyBackend {
    OpenAi,
    Anthropic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalExtractionChoice {
    Gemma4E4B,
    Gemma426BA4B,
    Skip,
}

impl ApiKeyBackend {
    fn extraction_model(self) -> &'static str {
        match self {
            Self::OpenAi => DEFAULT_EXTRACTION_MODEL,
            Self::Anthropic => DEFAULT_ANTHROPIC_MODEL,
        }
    }

    fn env_var(self) -> &'static str {
        match self {
            Self::OpenAi => "OPENAI_API_KEY",
            Self::Anthropic => "ANTHROPIC_API_KEY",
        }
    }
}

impl LocalExtractionChoice {
    fn model_id(self) -> Option<&'static str> {
        match self {
            Self::Gemma4E4B => Some(LOCAL_EXTRACTION_SMALL),
            Self::Gemma426BA4B => Some(LOCAL_EXTRACTION_LARGE),
            Self::Skip => None,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            splade_enabled: false,
            extraction_model: default_extraction_model(),
            embedding_model: default_embedding_model(),
            models_dir: default_models_dir(),
            user: None,
            alpha: default_alpha(),
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

fn default_models_dir() -> String {
    DEFAULT_MODELS_DIR.into()
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
    write_config_atomically(&path, &serde_json::to_vec_pretty(config)?)
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

    let path = config_path()?;
    let had_config = path.exists();
    let mut config = load_config()?;
    normalize_config(&mut config);

    let tty_available = interactive && io::stdin().is_terminal() && io::stdout().is_terminal();
    let api_backend = preferred_api_backend();
    let mut changed = !had_config;

    if let Some(backend) = api_backend {
        let preferred_model = backend.extraction_model();
        if config.extraction_model != preferred_model {
            println!(
                "ctx: found {}. using {} for extraction.",
                backend.env_var(),
                preferred_model
            );
            config.extraction_model = preferred_model.into();
            changed = true;
        }

        if !had_config {
            if fastembed_downloads_disabled() {
                println!(
                    "ctx: CTX_DISABLE_FASTEMBED=1. skipping fastembed downloads and leaving the dense fallback enabled."
                );
            } else {
                maybe_install_required_models(&config, tty_available);
            }
        }

        if changed {
            save_config(&config)?;
        }
        return Ok(config);
    }

    if needs_local_extraction_choice(&config) {
        configure_embedded_models(&mut config, tty_available)?;
        save_config(&config)?;
        return Ok(config);
    }

    if changed {
        save_config(&config)?;
    }

    Ok(config)
}

pub fn ensure_model_choice() -> Result<()> {
    ensure_local_setup(true).map(|_| ())
}

fn normalize_config(config: &mut Config) {
    if config.embedding_model.trim().is_empty() {
        config.embedding_model = DEFAULT_EMBEDDING_MODEL.into();
    }
    if config.models_dir.trim().is_empty() {
        config.models_dir = DEFAULT_MODELS_DIR.into();
    }
    if !(0.0..=1.0).contains(&config.alpha) {
        config.alpha = default_alpha();
    }
}

fn preferred_api_backend() -> Option<ApiKeyBackend> {
    if env_var_present("OPENAI_API_KEY") {
        Some(ApiKeyBackend::OpenAi)
    } else if env_var_present("ANTHROPIC_API_KEY") {
        Some(ApiKeyBackend::Anthropic)
    } else {
        None
    }
}

fn env_var_present(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn needs_local_extraction_choice(config: &Config) -> bool {
    !matches!(
        config.extraction_model.as_str(),
        LOCAL_EXTRACTION_SMALL | LOCAL_EXTRACTION_LARGE | UNCONFIGURED_EXTRACTION_MODEL
    )
}

fn configure_embedded_models(config: &mut Config, tty_available: bool) -> Result<()> {
    println!("ctx: no api keys found. setting up embedded models.\n");

    config.embedding_model = DEFAULT_EMBEDDING_MODEL.into();

    if fastembed_downloads_disabled() {
        println!(
            "ctx: CTX_DISABLE_FASTEMBED=1. skipping fastembed downloads and keeping the hash fallback enabled."
        );
        config.splade_enabled = false;
    } else {
        println!("downloading required models via fastembed:");
        println!("  all-MiniLM-L6-v2   86MB  (dense embedding, always required)");
        println!("  BGERerankerBase    86MB  (reranker, always required)");
        maybe_install_required_models(config, tty_available);
        println!();

        println!("optional: splade sparse retrieval (Splade_PP_en_v1, 532MB)");
        println!("  bridges vocabulary gaps — \"login\" finds \"auth service\"");
        let wants_splade = if tty_available {
            prompt_yes_no("  install?", false)?
        } else {
            false
        };
        config.splade_enabled = if wants_splade {
            maybe_install_splade_model(config, tty_available)
        } else {
            false
        };
    }

    println!();
    println!("no extraction model found.");
    let total_ram_gib = detected_ram_gib();
    let choice = if tty_available {
        prompt_local_extraction(total_ram_gib)?
    } else {
        default_noninteractive_choice(total_ram_gib)
    };

    config.extraction_model = choice
        .model_id()
        .unwrap_or(UNCONFIGURED_EXTRACTION_MODEL)
        .into();

    if let Some(model_id) = choice.model_id() {
        if tty_available {
            maybe_install_local_extraction_model(model_id, true);
        } else {
            eprintln!(
                "ctx: local extraction model {} will download on first use. rerun `ctx init` in a terminal to preinstall it now.",
                model_id
            );
        }
    }

    Ok(())
}

fn maybe_install_required_models(config: &Config, show_download_progress: bool) {
    let models_dir = resolve_models_dir(config);
    if let Err(error) = install_required_fastembed_assets(&models_dir, show_download_progress) {
        eprintln!(
            "ctx: warning: failed to preinstall required fastembed assets: {error}. local indexing will keep using the deterministic hash fallback until the models are available."
        );
    }
}

fn maybe_install_splade_model(config: &Config, show_download_progress: bool) -> bool {
    let models_dir = resolve_models_dir(config);
    match install_splade_asset(&models_dir, show_download_progress) {
        Ok(()) => true,
        Err(error) => {
            eprintln!(
                "ctx: warning: failed to install Splade_PP_en_v1: {error}. sparse retrieval stays disabled."
            );
            false
        }
    }
}

fn maybe_install_local_extraction_model(model_id: &str, show_download_progress: bool) {
    match ensure_local_extraction_model(model_id, show_download_progress) {
        Ok(path) => {
            if show_download_progress {
                println!("ctx: extraction model ready at {}", path.display());
            }
        }
        Err(error) => {
            eprintln!(
                "ctx: warning: failed to preinstall local extraction model {}: {error}. ctx will retry on first extraction and fall back to heuristic extraction if the model is still unavailable.",
                model_id
            );
        }
    }
}

fn fastembed_downloads_disabled() -> bool {
    std::env::var("CTX_DISABLE_FASTEMBED").ok().as_deref() == Some("1")
}

fn detected_ram_gib() -> u64 {
    let mut system = System::new();
    system.refresh_memory();
    system.total_memory() / 1024 / 1024 / 1024
}

fn default_noninteractive_choice(total_ram_gib: u64) -> LocalExtractionChoice {
    // Headless callers should get the smallest sane local default. Pulling the 8 GB
    // tier automatically would be a surprising side effect for CI or server boots.
    if total_ram_gib >= 8 {
        eprintln!(
            "ctx: no api keys found and no TTY available. defaulting extraction to {}. rerun `ctx init` in a terminal to choose a different local model.",
            LOCAL_EXTRACTION_SMALL
        );
        LocalExtractionChoice::Gemma4E4B
    } else {
        eprintln!(
            "ctx: no api keys found, no TTY is available, and only ~{}GB RAM was detected. leaving extraction unconfigured; set OPENAI_API_KEY or ANTHROPIC_API_KEY, or rerun `ctx init` in a terminal to choose a local model.",
            total_ram_gib
        );
        LocalExtractionChoice::Skip
    }
}

fn prompt_yes_no(prompt: &str, default: bool) -> Result<bool> {
    let default_hint = if default { "[Y/n]" } else { "[y/N]" };
    loop {
        print!("{prompt} {default_hint}: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        match input.trim().to_ascii_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("please answer y or n."),
        }
    }
}

fn prompt_local_extraction(total_ram_gib: u64) -> Result<LocalExtractionChoice> {
    println!("  [1] gemma4-e4b     ~3GB   recommended (8GB+ ram)");
    println!("  [2] gemma4-26b-a4b ~8GB   higher quality (16GB+ ram)");
    println!("  [3] skip — set OPENAI_API_KEY or ANTHROPIC_API_KEY instead");
    if total_ram_gib > 0 {
        println!("  detected RAM: ~{}GB", total_ram_gib);
    }

    loop {
        print!("  choose [1-3] (default 1): ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        match input.trim() {
            "" | "1" => return Ok(LocalExtractionChoice::Gemma4E4B),
            "2" => return Ok(LocalExtractionChoice::Gemma426BA4B),
            "3" => return Ok(LocalExtractionChoice::Skip),
            _ => println!("please choose 1, 2, or 3."),
        }
    }
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
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.json");
    let tmp = path.with_file_name(format!(
        ".{}.tmp-{}-{}",
        file_name,
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));

    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let mut options = fs::OpenOptions::new();
        options.create(true).write(true).truncate(true).mode(0o600);
        let mut file = options
            .open(&tmp)
            .with_context(|| format!("failed to create {}", tmp.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("failed to write {}", tmp.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to flush {}", tmp.display()))?;
        fs::rename(&tmp, path).with_context(|| format!("failed to replace {}", path.display()))?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to secure {}", path.display()))?;
    }

    #[cfg(not(unix))]
    {
        fs::write(&tmp, bytes).with_context(|| format!("failed to write {}", tmp.display()))?;
        fs::rename(&tmp, path).with_context(|| format!("failed to replace {}", path.display()))?;
    }

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
        let expected = dirs::home_dir()
            .expect("home directory")
            .join(".ctx/models");
        assert_eq!(expand_tilde("~/.ctx/models"), expected);
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
    fn default_config_uses_portable_model_path() {
        assert_eq!(Config::default().models_dir, "~/.ctx/models");
    }

    #[test]
    fn openai_defaults_need_local_choice_without_keys() {
        let config = Config::default();
        assert!(needs_local_extraction_choice(&config));
    }

    #[test]
    fn explicit_skip_is_not_reprompted() {
        let config = Config {
            extraction_model: UNCONFIGURED_EXTRACTION_MODEL.into(),
            ..Config::default()
        };
        assert!(!needs_local_extraction_choice(&config));
    }

    #[test]
    fn local_models_skip_reprompt() {
        let config = Config {
            extraction_model: LOCAL_EXTRACTION_SMALL.into(),
            ..Config::default()
        };
        assert!(!needs_local_extraction_choice(&config));
    }

    #[test]
    fn noninteractive_choice_prefers_small_model() {
        assert_eq!(
            default_noninteractive_choice(8),
            LocalExtractionChoice::Gemma4E4B
        );
        assert_eq!(
            default_noninteractive_choice(32),
            LocalExtractionChoice::Gemma4E4B
        );
        assert_eq!(
            default_noninteractive_choice(4),
            LocalExtractionChoice::Skip
        );
    }
}
