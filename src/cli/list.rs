use anyhow::Result;
use clap::Args;

const COLOR_RESET: &str = "\x1b[0m";
const COLOR_NAME: &str = "\x1b[36;1m";
const COLOR_META: &str = "\x1b[32m";

#[derive(Debug, Args)]
#[command(about = "List local contexts")]
pub struct ListArgs {}

pub async fn run(_args: ListArgs) -> Result<()> {
    let default_ctx = crate::install::default_context_selection();
    for name in crate::list_context_names()? {
        if default_ctx.as_ref().is_some_and(|d| d == &name) {
            println!("{COLOR_NAME}{name}{COLOR_RESET} {COLOR_META}(default){COLOR_RESET}");
        } else {
            println!("{COLOR_NAME}{name}{COLOR_RESET}");
        }
    }
    Ok(())
}
