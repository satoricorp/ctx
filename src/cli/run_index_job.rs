use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

use crate::index_job;

#[derive(Debug, Args)]
pub struct RunIndexJobArgs {
    #[arg(long)]
    pub job_path: PathBuf,
}

pub async fn run(args: RunIndexJobArgs) -> Result<()> {
    index_job::execute_index_job_file(&args.job_path).await
}
