use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
#[command(about = "Create a new local context")]
pub struct InitArgs {
    #[arg(value_name = "name")]
    pub name: Option<String>,
    /// Short description for this context.
    #[arg(short = 'd', long = "description", value_name = "text")]
    pub description: Option<String>,
    /// Run without prompts.
    #[arg(short = 'y', long = "yes")]
    pub yes: bool,
}

pub async fn run(args: InitArgs) -> Result<()> {
    let _ = args.yes;
    let name = args.name.unwrap_or(crate::artifact::infer_context_name()?);
    let status = crate::init_context_with_description(&name, args.description.as_deref()).await?;
    println!("initialized {}", status.name);
    Ok(())
}
