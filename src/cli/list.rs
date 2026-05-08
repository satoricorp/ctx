use anyhow::Result;
use clap::Args;

const COLOR_RESET: &str = "\x1b[0m";
const COLOR_NAME: &str = "\x1b[36;1m";
const COLOR_META: &str = "\x1b[32m";
const COLOR_DESC: &str = "\x1b[90m";

#[derive(Debug, Args)]
#[command(about = "List local contexts")]
pub struct ListArgs {}

pub async fn run(_args: ListArgs) -> Result<()> {
    let default_ctx = crate::install::default_context_selection();
    for context in crate::list_contexts()? {
        if default_ctx.as_ref().is_some_and(|d| d == &context.name) {
            println!(
                "{COLOR_NAME}{}{COLOR_RESET} {COLOR_META}(default){COLOR_RESET}",
                context.name
            );
        } else {
            println!("{COLOR_NAME}{}{COLOR_RESET}", context.name);
        }
        if let Some(description) = context.description.as_deref().map(str::trim) {
            if !description.is_empty() {
                println!("{COLOR_DESC}{description}{COLOR_RESET}");
            }
        }
    }
    Ok(())
}
