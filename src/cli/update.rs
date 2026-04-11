use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct UpdateArgs {
    #[arg(short = 'c', long = "context")]
    pub context: Option<String>,
}

pub async fn run(args: UpdateArgs) -> Result<()> {
    let context = args
        .context
        .unwrap_or(crate::artifact::infer_context_name()?);
    let status = crate::update_context(&context).await?;
    println!("updated {}", status.name);
    Ok(())
}
