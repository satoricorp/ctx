use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct InitArgs {
    #[arg(value_name = "name")]
    pub name: Option<String>,
}

pub async fn run(_args: InitArgs) -> Result<()> {
    anyhow::bail!("ctx init is not implemented yet")
}

