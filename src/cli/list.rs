use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct ListArgs {}

pub async fn run(_args: ListArgs) -> Result<()> {
    let default_ctx = crate::install::default_context_selection();
    for name in crate::list_context_names()? {
        if default_ctx.as_ref().is_some_and(|d| d == &name) {
            println!("* {name}");
        } else {
            println!("{name}");
        }
    }
    Ok(())
}
