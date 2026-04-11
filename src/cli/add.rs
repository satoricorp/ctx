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

pub async fn run(_args: AddArgs) -> Result<()> {
    anyhow::bail!("ctx add is not implemented yet")
}

