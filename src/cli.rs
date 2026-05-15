pub mod add;
pub mod doctor;
pub mod index_prompt;
pub mod init;
pub mod list;
pub mod mcp;
pub mod notes;
pub mod progress;
pub mod query;
pub mod remember;
pub mod run_index_job;
pub mod scope;
pub mod status;
pub mod theme;
pub mod update;
pub mod use_context;

use anyhow::Result;
use clap::{
    builder::{styling::AnsiColor, Styles},
    ArgAction, CommandFactory, Parser, Subcommand,
};

const CLI_STYLES: Styles = Styles::styled()
    .header(AnsiColor::White.on_default().bold())
    .usage(AnsiColor::White.on_default().bold())
    .literal(AnsiColor::Cyan.on_default().bold())
    .placeholder(AnsiColor::Green.on_default());

const CLI_HELP_TEMPLATE: &str = "{before-help}\n{usage-heading} {usage}\n\n{all-args}";

#[derive(Debug, Parser)]
#[command(name = "ctx")]
#[command(about = "local-first context runtime for agents")]
#[command(disable_version_flag = true)]
#[command(before_help = theme::CTX_BANNER)]
#[command(help_template = CLI_HELP_TEMPLATE)]
#[command(styles = CLI_STYLES)]
pub struct Cli {
    #[arg(
        short = 'v',
        long = "version",
        action = ArgAction::SetTrue,
        help = "Print version"
    )]
    version: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Create a new local context.
    Init(init::InitArgs),
    /// Ingest a file or directory into a context.
    Add(add::AddArgs),
    /// Save durable text memory into notes.
    Remember(remember::RememberArgs),
    /// Ask a context and get a cited answer.
    Query(query::QueryArgs),
    /// Re-index changed files and refresh notes.
    Update(update::UpdateArgs),
    /// Set the default context.
    Use(use_context::UseArgs),
    /// Open the context's notes directory.
    Notes(notes::NotesArgs),
    /// List local contexts.
    List(list::ListArgs),
    /// Show context status and active jobs.
    Status(status::StatusArgs),
    /// Check and optionally repair a context.
    Doctor(doctor::DoctorArgs),
    /// Start the local MCP server.
    Mcp(mcp::McpArgs),
    /// Internal: run a background indexing job from `run/job-*.json`.
    #[command(hide = true)]
    RunIndexJob(run_index_job::RunIndexJobArgs),
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    if cli.version {
        println!("{}", theme::success("ctx", env!("CARGO_PKG_VERSION")));
        return Ok(());
    }

    let Some(command) = cli.command else {
        collect_signup_blocking()?;
        Cli::command().print_help()?;
        println!();
        return Ok(());
    };

    match &command {
        Commands::RunIndexJob(_) | Commands::Mcp(_) => {}
        _ => {
            collect_signup_blocking()?;
        }
    }

    match command {
        Commands::Init(args) => init::run(args).await,
        Commands::Add(args) => add::run(args).await,
        Commands::Remember(args) => remember::run(args).await,
        Commands::Query(args) => query::run(args).await,
        Commands::Update(args) => update::run(args).await,
        Commands::Use(args) => use_context::run(args).await,
        Commands::Notes(args) => notes::run(args).await,
        Commands::List(args) => list::run(args).await,
        Commands::Status(args) => status::run(args).await,
        Commands::Doctor(args) => doctor::run(args).await,
        Commands::Mcp(args) => mcp::run(args).await,
        Commands::RunIndexJob(args) => run_index_job::run(args).await,
    }
}

fn collect_signup_blocking() -> Result<()> {
    tokio::task::block_in_place(crate::signup::maybe_collect_signup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn detects_short_and_long_version_requests() {
        let short = Cli::try_parse_from(["ctx", "-v"]).expect("short version should parse");
        assert!(short.version);
        assert!(short.command.is_none());

        let long = Cli::try_parse_from(["ctx", "--version"]).expect("long version should parse");
        assert!(long.version);
        assert!(long.command.is_none());
    }

    #[test]
    fn query_parse_does_not_require_version() {
        let cli = Cli::try_parse_from(["ctx", "query", "whats my name"])
            .expect("query should parse without version flag");

        assert!(matches!(cli.command, Some(Commands::Query(_))));
    }
}
