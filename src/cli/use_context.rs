use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
#[command(about = "Set the default context")]
pub struct UseArgs {
    #[arg(value_name = "context")]
    pub context: String,
}

pub async fn run(args: UseArgs) -> Result<()> {
    crate::install::set_default_context(&args.context)?;
    println!("default context set to {}", args.context.trim());
    Ok(())
}
