use anyhow::Result;
use clap::Args;

use crate::cli::scope::apply_context_image_flag;

#[derive(Debug, Args)]
pub struct InitArgs {
    #[arg(value_name = "name")]
    pub name: Option<String>,
    /// Same as env **CTX_IMAGE**: store this context under `…/contexts/images/<tag>/`.
    #[arg(long = "image", value_name = "TAG_OR_DIGEST")]
    pub image: Option<String>,
    /// Run the embedded-model wizard (FastEmbed assets, optional Splade, Gemma tier) and download
    /// the chosen local extraction GGUF. Use when `ctx init` skipped this because your config
    /// already listed a local model or `unconfigured`. Requires `OPENAI_API_KEY` and
    /// `ANTHROPIC_API_KEY` unset for this command.
    #[arg(long = "setup-models")]
    pub setup_models: bool,
    /// Non-interactive: accept recommended defaults (skip optional Splade; install **gemma4-e4b**
    /// when detected RAM ≥ 8GB, else leave extraction unconfigured). For use by agents and CI.
    #[arg(short = 'y', long = "yes")]
    pub yes: bool,
}

pub async fn run(args: InitArgs) -> Result<()> {
    apply_context_image_flag(&args.image);
    crate::install::ensure_model_choice(args.setup_models, args.yes)?;

    let name = args.name.unwrap_or(crate::artifact::infer_context_name()?);
    let status = crate::init_context(&name).await?;
    println!("initialized {}", status.name);
    Ok(())
}
