use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct PublishArgs {
    #[arg(short = 'c', long = "context")]
    pub context: Option<String>,
}

pub async fn run(_args: PublishArgs) -> Result<()> {
    anyhow::bail!("ctx publish is not implemented yet")
}

