use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct StatusArgs {
    #[arg(short = 'c', long = "context")]
    pub context: Option<String>,
}

pub async fn run(_args: StatusArgs) -> Result<()> {
    anyhow::bail!("ctx status is not implemented yet")
}

