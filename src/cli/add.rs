use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

use crate::cli::scope::{apply_context_image_flag, ContextSelectArgs};
use crate::extraction::classifier::ContentLayer;

#[derive(Debug, Args)]
pub struct AddArgs {
    #[command(flatten)]
    pub select: ContextSelectArgs,
    #[arg(value_name = "path")]
    pub path: PathBuf,
    #[arg(long = "type")]
    pub layer: Option<ContentLayer>,
}

pub async fn run(args: AddArgs) -> Result<()> {
    apply_context_image_flag(&args.select.image);
    let context = args
        .select
        .context
        .clone()
        .unwrap_or(crate::artifact::infer_context_name()?);
    let outcome = crate::add_to_context(&context, &args.path, args.layer).await?;
    println!(
        "indexed {} chunks {} entities",
        outcome.chunks_written, outcome.entities_written
    );
    Ok(())
}
