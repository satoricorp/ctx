use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct InitArgs {
    #[arg(value_name = "name")]
    pub name: Option<String>,
    /// Non-interactive. Reserved for scripts and CI.
    #[arg(short = 'y', long = "yes")]
    pub yes: bool,
}

pub async fn run(args: InitArgs) -> Result<()> {
    let _ = args.yes;
    let name = args.name.unwrap_or(crate::artifact::infer_context_name()?);
    let status = crate::init_context(&name).await?;
    println!("initialized {}", status.name);
    Ok(())
}
