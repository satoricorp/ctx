use anyhow::Result;
use clap::Args;

use crate::cli::scope::{apply_context_image_flag, ContextSelectArgs};

#[derive(Debug, Args)]
pub struct UpdateArgs {
    #[command(flatten)]
    pub select: ContextSelectArgs,
}

pub async fn run(args: UpdateArgs) -> Result<()> {
    apply_context_image_flag(&args.select.image);
    let context = args
        .select
        .context
        .clone()
        .unwrap_or(crate::artifact::infer_context_name()?);
    let status = crate::update_context(&context).await?;
    println!("updated {}", status.name);
    Ok(())
}
