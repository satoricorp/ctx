use anyhow::{bail, Context, Result};
use clap::Args;
use std::fs;
use std::path::PathBuf;

use crate::cli::progress::CliSpinner;
use crate::cli::scope::{resolve_context_name, ContextSelectArgs};
use crate::cli::theme;
use crate::ensure_context_for_add;
use crate::extraction::classifier::ContentLayer;
use crate::index_job::{self, BackgroundJobLaunch, IndexJobKind};
use crate::index_plan;

#[derive(Debug, Args)]
#[command(about = "Queue a background indexing job for a file or directory")]
pub struct AddArgs {
    #[command(flatten)]
    pub select: ContextSelectArgs,
    #[arg(value_name = "path")]
    pub path: PathBuf,
    /// Force semantic or procedural indexing.
    #[arg(long = "type")]
    pub layer: Option<ContentLayer>,
    /// Also store raw source bytes under `blobs/sha256/` and flip
    /// `manifest.config.store_raw_content` on for this artifact.
    #[arg(long = "with-content")]
    pub with_content: bool,
    /// Print planned files and time estimate, then exit without indexing.
    #[arg(long = "dry-run")]
    pub dry_run: bool,
}

pub async fn run(args: AddArgs) -> Result<()> {
    #[cfg(not(unix))]
    bail!("`ctx add` uses background indexing and is currently supported on Unix (macOS, Linux)");

    let context = resolve_context_name(&args.select)?;
    let ctx_path = ensure_context_for_add(&context)?;

    let abs = args
        .path
        .canonicalize()
        .with_context(|| format!("resolve {}", args.path.display()))?;

    let plan = if abs.is_dir() {
        index_plan::plan_add_directory(&ctx_path, &abs, args.layer, args.with_content)?
    } else {
        let root = abs
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        let rel = PathBuf::from(
            abs.file_name()
                .with_context(|| format!("path has no file name: {}", abs.display()))?,
        );
        index_plan::plan_add_single_file(
            &ctx_path,
            &root,
            &rel,
            &abs,
            args.layer,
            args.with_content,
        )?
    };

    let est = index_plan::estimate_seconds(&plan);

    if args.dry_run {
        eprintln!(
            "{} {}",
            theme::section("Plan"),
            index_plan::describe_plan_line(&plan)
        );
        eprintln!(
            "{}",
            theme::key_value(
                "estimate",
                format!(
                    "{} (rough; model and network dependent)",
                    index_plan::format_duration_humans(est)
                )
            )
        );
        return Ok(());
    }

    let spinner = CliSpinner::new("scheduling background indexing");
    if let Some(job) = index_job::resume_background_index_job(&ctx_path)? {
        spinner.success(background_job_message("resumed", &context, &job));
        return Ok(());
    }

    if plan.is_empty() {
        if !abs.is_dir() {
            if plan.stats.skipped_too_large > 0 {
                let meta = fs::metadata(&abs)?;
                bail!("file size {} exceeds binary decoder cap", meta.len());
            }
            if plan.stats.skipped_stat_error > 0 {
                bail!("could not read {}", abs.display());
            }
        }
        spinner.success(theme::headline("index current", &context));
        return Ok(());
    }

    let job = index_job::start_background_index_job(&context, &plan, IndexJobKind::Add, false)?;
    spinner.success(background_job_message("started", &context, &job));
    Ok(())
}

fn background_job_message(action: &str, context: &str, job: &BackgroundJobLaunch) -> String {
    theme::headline_detail(
        format!("indexing {action}"),
        context,
        format!(
            "job {} · pid {} · progress {}",
            job.job_id,
            job.pid,
            theme::command("ctx status")
        ),
    )
}
