use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct PullArgs {
    #[arg(value_name = "name[@version]")]
    pub name: String,
}

pub async fn run(_args: PullArgs) -> Result<()> {
    anyhow::bail!("ctx pull is not implemented yet")
}

