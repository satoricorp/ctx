use anyhow::Result;
use clap::Args;

use crate::retrieval::query::QueryType;

#[derive(Debug, Args)]
pub struct QueryArgs {
    #[arg(value_name = "text")]
    pub query: String,
    #[arg(short = 'c', long = "context")]
    pub context: Option<String>,
    #[arg(long = "type", default_value_t)]
    pub kind: QueryType,
    #[arg(long = "k", default_value_t = 5)]
    pub k: usize,
}

pub async fn run(_args: QueryArgs) -> Result<()> {
    anyhow::bail!("ctx query is not implemented yet")
}

