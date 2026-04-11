use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct InitArgs {
    #[arg(value_name = "name")]
    pub name: Option<String>,
}

pub async fn run(args: InitArgs) -> Result<()> {
    let name = args.name.unwrap_or(crate::artifact::infer_context_name()?);
    let status = crate::init_context(&name).await?;
    println!("initialized {}", status.name);
    Ok(())
}
