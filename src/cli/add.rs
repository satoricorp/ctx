use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

use crate::extraction::classifier::ContentLayer;

#[derive(Debug, Args)]
pub struct AddArgs {
    #[arg(value_name = "path")]
    pub path: PathBuf,
    #[arg(short = 'c', long = "context")]
    pub context: Option<String>,
    #[arg(long = "type")]
    pub layer: Option<ContentLayer>,
}

pub async fn run(args: AddArgs) -> Result<()> {
    let context = args.context.unwrap_or(crate::artifact::infer_context_name()?);
    let outcome = crate::add_to_context(&context, &args.path, args.layer).await?;
    println!(
        "indexed {} chunks {} entities",
        outcome.chunks_written, outcome.entities_written
    );
    Ok(())
}
