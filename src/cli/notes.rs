use anyhow::{Context, Result};
use clap::Args;

use crate::artifact::notes_path;
use crate::cli::scope::{resolve_context_name, ContextSelectArgs};

#[derive(Debug, Args)]
pub struct NotesArgs {
    #[command(flatten)]
    pub select: ContextSelectArgs,
}

pub async fn run(args: NotesArgs) -> Result<()> {
    let context = resolve_context_name(&args.select)?;
    let ctx_path = crate::open_existing_context(&context)?;
    let dir = notes_path(&ctx_path);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create notes directory {}", dir.display()))?;
    open::that(&dir).with_context(|| format!("open notes directory {}", dir.display()))?;
    println!("{}", dir.display());
    Ok(())
}
