pub mod add;
pub mod doctor;
pub mod index_prompt;
pub mod init;
pub mod list;
pub mod mcp;
pub mod notes;
pub mod publish;
pub mod pull;
pub mod query;
pub mod run_index_job;
pub mod scope;
pub mod status;
pub mod use_context;
pub mod update;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "ctx")]
#[command(about = "local-first context runtime for agents")]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Init(init::InitArgs),
    Add(add::AddArgs),
    Query(query::QueryArgs),
    Update(update::UpdateArgs),
    Use(use_context::UseArgs),
    Notes(notes::NotesArgs),
    List(list::ListArgs),
    Status(status::StatusArgs),
    Doctor(doctor::DoctorArgs),
    Mcp(mcp::McpArgs),
    Publish(publish::PublishArgs),
    Pull(pull::PullArgs),
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
        Commands::Publish(args) => publish::run(args).await,
        Commands::Pull(args) => pull::run(args).await,
        Commands::RunIndexJob(args) => run_index_job::run(args).await,
    }
}
