use anyhow::Result;
use clap::Args;

use crate::cli::scope::{resolve_context_name, ContextSelectArgs};

#[derive(Debug, Args)]
pub struct UpdateArgs {
    #[command(flatten)]
    pub select: ContextSelectArgs,
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,
}

pub async fn run(args: UpdateArgs) -> Result<()> {
    let context = resolve_context_name(&args.select)?;
    let status = crate::update_context_with_verbosity(&context, args.verbose).await?;
    println!("updated {}", status.name);
    Ok(())
}
