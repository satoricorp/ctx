pub mod add;
pub mod doctor;
pub mod index_prompt;
pub mod init;
pub mod list;
pub mod mcp;
pub mod notes;
pub mod progress;
pub mod query;
pub mod run_index_job;
pub mod scope;
pub mod status;
pub mod update;
pub mod use_context;

use anyhow::Result;
use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser, Subcommand,
};

const CLI_STYLES: Styles = Styles::styled()
    .header(AnsiColor::Yellow.on_default().bold())
    .usage(AnsiColor::Yellow.on_default().bold())
    .literal(AnsiColor::Cyan.on_default().bold())
    .placeholder(AnsiColor::Green.on_default());

#[derive(Debug, Parser)]
#[command(name = "ctx")]
#[command(about = "local-first context runtime for agents")]
#[command(styles = CLI_STYLES)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Create a new local context.
    Init(init::InitArgs),
    /// Queue a background indexing job for a file or directory.
    Add(add::AddArgs),
    /// Search indexed content and synthesize an answer.
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
    match Cli::parse().command {
        Commands::Init(args) => init::run(args).await,
        Commands::Add(args) => add::run(args).await,
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
