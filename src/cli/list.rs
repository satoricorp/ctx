use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct ListArgs;

pub async fn run(_args: ListArgs) -> Result<()> {
    anyhow::bail!("ctx list is not implemented yet")
}

