use anyhow::Result;
use clap::Args;

use crate::cli::theme;

#[derive(Debug, Args)]
#[command(about = "List local contexts")]
pub struct ListArgs {}

pub async fn run(_args: ListArgs) -> Result<()> {
    let default_ctx = crate::install::default_context_selection();
    for context in crate::list_contexts()? {
        if default_ctx.as_ref().is_some_and(|d| d == &context.name) {
            println!(
                "{} {}",
                theme::command(&context.name),
                theme::pill("default")
            );
        } else {
            println!("{}", theme::command(&context.name));
        }
        if let Some(description) = context.description.as_deref().map(str::trim) {
            if !description.is_empty() {
                println!("{}", theme::muted(description));
            }
        }
    }
    Ok(())
}
