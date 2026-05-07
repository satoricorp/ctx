use anyhow::Result;
use clap::Args;

use crate::artifact::context_path;
use crate::artifact::Manifest;
use crate::cli::progress::CliSpinner;
use crate::cli::scope::ContextSelectArgs;
use crate::index_job::{self, IndexJobKind};
use crate::index_plan;

const COLOR_RESET: &str = "\x1b[0m";
const COLOR_NAME: &str = "\x1b[36;1m";
const COLOR_COUNT: &str = "\x1b[33;1m";
const COLOR_META: &str = "\x1b[32m";
const COLOR_LABEL: &str = "\x1b[90m";
const COLOR_MODEL: &str = "\x1b[35m";

#[derive(Debug, Args)]
#[command(about = "Show context status and active jobs")]
pub struct StatusArgs {
    #[command(flatten)]
    pub select: ContextSelectArgs,
}

pub async fn run(args: StatusArgs) -> Result<()> {
    let default_ctx = crate::install::default_context_selection();
    let default_ref = default_ctx.as_deref();

    if let Some(name) = args
        .select
        .context
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let spinner = CliSpinner::new("loading status");
        let summary = load_status_summary(name)?;
        spinner.success("status loaded");
        return print_one_context_status(&summary, default_ref);
    }

    let names = crate::list_context_names()?;
    if names.is_empty() {
        return Ok(());
    }

    let spinner = CliSpinner::new("loading status");
    let mut summaries = Vec::with_capacity(names.len());
    for name in names {
        summaries.push(load_status_summary(&name)?);
    }
    spinner.success("status loaded");

    let mut first = true;
    for summary in summaries {
        if !first {
            println!();
        }
        first = false;
        print_one_context_status(&summary, default_ref)?;
    }
    Ok(())
}

struct StatusSummary {
    name: String,
    indexed_count: usize,
    extraction_model: String,
    active_job: Option<index_job::IndexJobState>,
}

fn load_status_summary(context: &str) -> Result<StatusSummary> {
    let ctx_path = context_path(context);
    let manifest = Manifest::load(&ctx_path)?;
    let indexed_count = manifest
        .sources
        .iter()
        .flat_map(|source| source.files.iter())
        .filter(|entry| entry.hash == entry.hash_at_index)
        .count();
    let active_job = index_job::read_active_job(&ctx_path)?;

    Ok(StatusSummary {
        name: manifest.name,
        indexed_count,
        extraction_model: manifest.config.extraction_model,
        active_job,
    })
}

fn print_one_context_status(status: &StatusSummary, default_ctx: Option<&str>) -> Result<()> {
    let star = default_ctx.is_some_and(|d| d == status.name.as_str());
    let default_suffix = if star {
        format!(" {COLOR_META}(default){COLOR_RESET}")
    } else {
        String::new()
    };
    println!(
        "{COLOR_NAME}{}{COLOR_RESET} {COLOR_LABEL}(indexed: {COLOR_COUNT}{}{COLOR_LABEL}){COLOR_RESET}{}",
        status.name,
        status.indexed_count,
        default_suffix,
    );
    println!(
        "{COLOR_LABEL}model:{COLOR_RESET}{COLOR_MODEL}{}{COLOR_RESET}",
        status.extraction_model
    );

    if let Some(job) = &status.active_job {
        if job_visible_in_status(&job) {
            print_index_job_line(&job);
        }
    }

    Ok(())
}

fn job_visible_in_status(job: &index_job::IndexJobState) -> bool {
    match job.phase.as_str() {
        "queued" => true,
        "running" => index_job::pid_alive(job.pid),
        _ => false,
    }
}

fn print_index_job_line(job: &index_job::IndexJobState) {
    let kind = match job.kind {
        IndexJobKind::Add => "add",
        IndexJobKind::Update => "update",
    };
    let pct = if job.total > 0 {
        (job.done as f64 / job.total as f64) * 100.0
    } else {
        100.0
    };

    let state = if job.phase == "queued" {
        "queued"
    } else if job.phase == "running" {
        "running"
    } else {
        job.phase.as_str()
    };

    println!(
        "index job ({kind}): {} — {}/{} files ({:.0}%), pid {}",
        state, job.done, job.total, pct, job.pid
    );
    if let Some(ref p) = job.current_path {
        if job.phase == "running" {
            println!("  current: {}", p);
        }
    }
    if let Some(ref e) = job.last_error {
        println!("  error: {}", e);
    }

    if job.total > job.done && job.done > 0 && job.phase == "running" {
        if let Some(stats) = index_plan::load_indexing_stats() {
            let mib_left: f64 = job
                .items
                .iter()
                .skip(job.done)
                .map(|i| i.size_bytes as f64)
                .sum::<f64>()
                / (1024.0 * 1024.0);
            if mib_left > 0.001 && stats.ema_secs_per_mib_semantic > 0.0 {
                let est = mib_left * stats.ema_secs_per_mib_semantic;
                println!(
                    "  eta (very rough): {}",
                    index_plan::format_duration_humans(est)
                );
            }
        }
    }
}
