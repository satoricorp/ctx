use anyhow::Result;
use clap::Args;

use crate::cli::index_prompt::{self, IndexRunChoice};
use crate::cli::scope::{resolve_context_name, ContextSelectArgs};
use crate::cli::theme;
use crate::index_job::{self, IndexJobKind};
use crate::index_plan;
use crate::{open_existing_context, update_context_with_verbosity};

#[derive(Debug, Args)]
#[command(about = "Re-index changed files and refresh notes")]
pub struct UpdateArgs {
    #[command(flatten)]
    pub select: ContextSelectArgs,
    /// Print file-by-file progress.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,
    /// Run synchronously without prompts.
    #[arg(short = 'y', long = "yes", conflicts_with = "background")]
    pub yes: bool,
    /// Same as `--yes`.
    #[arg(
        long = "no-interactive",
        alias = "non-interactive",
        conflicts_with = "background"
    )]
    pub no_interactive: bool,
    /// Detach a worker process (Unix only). Check progress with `ctx status`.
    #[arg(short = 'b', long = "background", conflicts_with = "yes")]
    pub background: bool,
    /// Print the planned work and estimate, then exit.
    #[arg(long = "dry-run")]
    pub dry_run: bool,
}

pub async fn run(args: UpdateArgs) -> Result<()> {
    let context = resolve_context_name(&args.select)?;
    let ctx_path = open_existing_context(&context)?;

    if !args.dry_run {
        index_job::try_finish_resumable_job(&ctx_path).await?;
    } else if index_job::read_active_job(&ctx_path)?
        .as_ref()
        .map(index_job::job_is_resumable)
        .unwrap_or(false)
    {
        eprintln!(
            "{}",
            theme::warn("interrupted index job pending; omit --dry-run to resume it first")
        );
    }

    let manifest = crate::artifact::Manifest::load(&ctx_path)?;
    let with_content = manifest.config.store_raw_content;

    let plan = index_plan::plan_manifest_update(&ctx_path, with_content)?;
    let est = index_plan::estimate_seconds(&plan);
    let non_interactive = args.yes || args.no_interactive;

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

    if args.background {
        #[cfg(not(unix))]
        anyhow::bail!("`--background` is only supported on Unix (macOS, Linux). Omit it to run synchronously.");
        #[cfg(unix)]
        {
            if plan.is_empty() {
                let status = crate::finalize_update_context(&context, &ctx_path).await?;
                println!(
                    "{}",
                    theme::success_detail(
                        "updated",
                        status.name,
                        "index already current · notes refreshed"
                    )
                );
                return Ok(());
            }
            let job =
                index_job::start_background_index_job(&context, &plan, IndexJobKind::Update, true)?;
            print_background_job(&job);
            return Ok(());
        }
    }

    let choice = if plan.is_empty() {
        IndexRunChoice::Sync
    } else if index_prompt::should_offer_choice(&plan, est, non_interactive, false, false) {
        index_prompt::prompt_run_mode(&plan, est)?
    } else {
        IndexRunChoice::Sync
    };

    match choice {
        IndexRunChoice::Cancel => {
            eprintln!("{}", theme::muted("cancelled"));
            Ok(())
        }
        IndexRunChoice::Background => {
            #[cfg(not(unix))]
            {
                anyhow::bail!(
                    "background indexing requires Unix; use --yes for non-interactive sync"
                );
            }
            #[cfg(unix)]
            {
                let job = index_job::start_background_index_job(
                    &context,
                    &plan,
                    IndexJobKind::Update,
                    true,
                )?;
                print_background_job(&job);
                Ok(())
            }
        }
        IndexRunChoice::Sync => {
            if plan.is_empty() {
                let status = update_context_with_verbosity(&context, args.verbose).await?;
                println!("{}", theme::success("updated", status.name));
                return Ok(());
            }
            let out = index_job::run_sync_with_progress(
                &context,
                &plan,
                IndexJobKind::Update,
                true,
                args.verbose,
            )
            .await?;
            let name = out
                .context_status
                .map(|s| s.name)
                .unwrap_or_else(|| context.clone());
            println!("{}", theme::success("updated", name));
            Ok(())
        }
    }
}

#[cfg(unix)]
fn print_background_job(job: &index_job::BackgroundJobLaunch) {
    println!(
        "{}",
        theme::success_detail(
            "indexing started",
            "background",
            format!("job {} · pid {}", job.job_id, job.pid)
        )
    );
    println!("{}", theme::key_value("log", job.log_path.display()));
    println!(
        "{}",
        theme::key_value("progress", theme::command("ctx status"))
    );
}
