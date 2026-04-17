use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

use crate::cli::scope::{resolve_context_name, ContextSelectArgs};
use crate::extraction::classifier::ContentLayer;

#[derive(Debug, Args)]
pub struct AddArgs {
    #[command(flatten)]
    pub select: ContextSelectArgs,
    #[arg(value_name = "path")]
    pub path: PathBuf,
    #[arg(long = "type")]
    pub layer: Option<ContentLayer>,
    /// Also store raw source bytes under `blobs/sha256/` and flip
    /// `manifest.config.store_raw_content` on for this artifact.
    #[arg(long = "with-content")]
    pub with_content: bool,
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,
}

pub async fn run(args: AddArgs) -> Result<()> {
    let context = resolve_context_name(&args.select)?;
    let outcome = crate::add_to_context_with_verbosity(
        &context,
        &args.path,
        args.layer,
        args.with_content,
        args.verbose,
    )
    .await?;
    println!(
        "indexed {} chunks {} entities",
        outcome.chunks_written, outcome.entities_written
    );
    Ok(())
}
