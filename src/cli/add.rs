use anyhow::{bail, Context, Result};
use clap::Args;
use std::fs;
use std::path::PathBuf;

use crate::cli::index_prompt::{self, IndexRunChoice};
use crate::cli::scope::{resolve_context_name, ContextSelectArgs};
use crate::extraction::classifier::ContentLayer;
use crate::index_job::{self, IndexJobKind};
use crate::index_plan;
use crate::ensure_context_for_add;

#[derive(Debug, Args)]
pub struct AddArgs {
    #[command(flatten)]
    pub select: ContextSelectArgs,
    #[arg(value_name = "path")]
    pub path: PathBuf,
    #[arg(long = "type")]
    pub layer: Option<ContentLayer>,
    /// Also store raw source bytes under `blobs/sha256/` and flip
    /// `manifest.config.store_raw_content` on for this artifact.
    #[arg(long = "with-content")]
    pub with_content: bool,
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,
    /// Do not prompt; run synchronously in this terminal.
    #[arg(short = 'y', long = "yes", conflicts_with = "background")]
    pub yes: bool,
    /// Same as `--yes` (for scripts and CI).
    #[arg(long = "no-interactive", alias = "non-interactive", conflicts_with = "background")]
    pub no_interactive: bool,
    /// Detach a worker process (Unix only). Progress: `ctx status`.
    #[arg(short = 'b', long = "background", conflicts_with = "yes")]
    pub background: bool,
    /// Print planned files and time estimate, then exit without indexing.
    #[arg(long = "dry-run")]
    pub dry_run: bool,
}

pub async fn run(args: AddArgs) -> Result<()> {
    let context = resolve_context_name(&args.select)?;
    let ctx_path = ensure_context_for_add(&context)?;
    let abs = args
        .path
        .canonicalize()
        .with_context(|| format!("resolve {}", args.path.display()))?;

    let non_interactive = args.yes || args.no_interactive;

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
        eprintln!("{}", index_plan::describe_plan_line(&plan));
        eprintln!(
            "estimate: {} (rough; model and network dependent)",
            index_plan::format_duration_humans(est)
        );
        return Ok(());
    }

    if plan.is_empty() {
        if !abs.is_dir() {
            if plan.stats.skipped_too_large > 0 {
                let meta = fs::metadata(&abs)?;
                bail!(
                    "file size {} exceeds binary decoder cap",
                    meta.len()
                );
            }
            if plan.stats.skipped_stat_error > 0 {
                bail!("could not read {}", abs.display());
            }
        }
        println!("nothing to index (all files already up to date)");
        println!("indexed 0 chunks 0 entities");
        return Ok(());
    }

    if args.background {
        #[cfg(not(unix))]
        bail!("`--background` is only supported on Unix (macOS, Linux). Omit it to index synchronously.");
        #[cfg(unix)]
        {
            index_job::start_background_index_job(
                &context,
                &plan,
                IndexJobKind::Add,
                false,
            )?;
            return Ok(());
        }
    }

    let choice = if index_prompt::should_offer_choice(&plan, est, non_interactive, false, false) {
        index_prompt::prompt_run_mode(&plan, est)?
    } else {
        IndexRunChoice::Sync
    };

    match choice {
        IndexRunChoice::Cancel => {
            eprintln!("cancelled");
            Ok(())
        }
        IndexRunChoice::Background => {
            #[cfg(not(unix))]
            {
                bail!("background indexing requires Unix; choose sync or run without interactive prompt (--yes)");
            }
            #[cfg(unix)]
            {
                index_job::start_background_index_job(
                    &context,
                    &plan,
                    IndexJobKind::Add,
                    false,
                )?;
                Ok(())
            }
        }
        IndexRunChoice::Sync => {
            let out = index_job::run_sync_with_progress(
                &context,
                &plan,
                IndexJobKind::Add,
                false,
                args.verbose,
            )
            .await?;
            println!(
                "indexed {} chunks {} entities",
                out.ingestion.chunks_written, out.ingestion.entities_written
            );
            Ok(())
        }
    }
}
