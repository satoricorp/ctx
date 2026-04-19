use anyhow::Result;
use clap::Args;

use crate::artifact::context_path;
use crate::cli::scope::ContextSelectArgs;
use crate::index_job::{self, IndexJobKind};
use crate::index_plan;

#[derive(Debug, Args)]
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
        return print_one_context_status(name, default_ref);
    }

    let names = crate::list_context_names()?;
    if names.is_empty() {
        return Ok(());
    }

    let mut first = true;
    for name in names {
        if !first {
            println!();
        }
        first = false;
        print_one_context_status(&name, default_ref)?;
    }
    Ok(())
}

fn print_one_context_status(context: &str, default_ctx: Option<&str>) -> Result<()> {
    let status = crate::context_status(context)?;
    let star = default_ctx.is_some_and(|d| d == status.name.as_str());
    println!(
        "context {}{}",
        status.name,
        if star { " *" } else { "" }
    );
    println!(
        "indexed {} dirty {} pending {}",
        status.indexed_count, status.dirty_count, status.pending_count
    );
    println!(
        "chunks {} entities {} relations {} procedures {}",
        status.counts.chunk_count,
        status.counts.entity_count,
        status.counts.relation_count,
        status.counts.procedure_count,
    );
    println!("extraction {}", status.extraction_model);
    println!("embedding {}", status.embedding_model);
    println!(
        "splade {}",
        if status.splade_enabled {
            "enabled"
        } else {
            "disabled"
        }
    );

    let ctx_path = context_path(context);
    if let Some(job) = index_job::read_active_job(&ctx_path)? {
        print_index_job_line(&job);
    }

    if !status.drifted_files.is_empty() {
        println!("drifted:");
        for path in &status.drifted_files {
            println!("  {}", path);
        }
        println!("hint: run `ctx update` to re-index");
    }

    Ok(())
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

    let live = index_job::pid_alive(job.pid);
    let state = if job.phase == "completed" {
        "completed"
    } else if job.phase == "failed" {
        "failed"
    } else if job.phase == "queued" {
        "queued"
    } else if job.phase == "running" && live {
        "running"
    } else if job.phase == "running" && !live {
        "interrupted (process ended; re-run `ctx add` or `ctx update` to resume)"
    } else {
        job.phase.as_str()
    };

    println!(
        "index job ({kind}): {} — {}/{} files ({:.0}%), pid {}",
        state, job.done, job.total, pct, job.pid
    );
    if let Some(ref p) = job.current_path {
        if job.phase == "running" && live {
            println!("  current: {}", p);
        }
    }
    if let Some(ref e) = job.last_error {
        println!("  error: {}", e);
    }
    println!("  log: {}", job.log_path.display());

    if job.total > job.done && job.done > 0 && job.phase == "running" && live {
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
