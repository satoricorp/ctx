use anyhow::{bail, Result};
use clap::Args;

use crate::artifact::{context_path, infer_context_name};

#[derive(Debug, Args)]
pub struct ContextSelectArgs {
    /// Context name. Defaults to the active context or current directory when possible.
    #[arg(
        short = 'c',
        long = "context",
        value_name = "name",
        help_heading = "Context"
    )]
    pub context: Option<String>,
}

/// Resolve context in precedence order:
/// 1) `--context` / `-c`
/// 2) env **CTX_IMAGE**
/// 3) config default (`~/.ctx/config.json` -> `default_context`)
/// 4) inferred context from cwd basename, only if that context already exists
pub fn resolve_context_name(select: &ContextSelectArgs) -> Result<String> {
    if let Some(explicit) = select
        .context
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Ok(explicit.to_string());
    }

    if let Ok(env_context) = std::env::var("CTX_IMAGE") {
        let env_context = env_context.trim();
        if !env_context.is_empty() {
            return Ok(env_context.to_string());
        }
    }

    let config = crate::install::load_config()?;
    if let Some(default_context) = config
        .default_context
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Ok(default_context.to_string());
    }

    let inferred = infer_context_name()?;
    if context_path(&inferred).exists() {
        return Ok(inferred);
    }
    bail!(
        "no context selected: directory name {:?} does not match an existing context ({}).\n\
         Use `ctx use <name>` or pass `-c` / `--context <name>`.",
        inferred,
        context_path(&inferred).display()
    );
}
