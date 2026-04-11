use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct UpdateArgs {
    #[arg(short = 'c', long = "context")]
    pub context: Option<String>,
}

pub async fn run(_args: UpdateArgs) -> Result<()> {
    anyhow::bail!("ctx update is not implemented yet")
}

