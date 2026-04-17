use anyhow::{anyhow, Result};
use clap::Args;
use tokio::sync::mpsc;

use crate::cli::scope::{resolve_context_name, ContextSelectArgs};
use crate::doctor::{run_doctor, tier_label, CheckReport};

#[derive(Debug, Args)]
pub struct DoctorArgs {
    #[command(flatten)]
    pub select: ContextSelectArgs,
    /// Apply non-destructive repairs (prune orphan blobs, sync aura registry,
    /// relocate stray root-level topics, rebuild the index if needed).
    #[arg(long = "fix")]
    pub fix: bool,
    /// Emit a machine-readable JSON report instead of streaming text output.
    #[arg(long = "json")]
    pub json: bool,
}

pub async fn run(args: DoctorArgs) -> Result<()> {
    let context = resolve_context_name(&args.select)?;

    let (tx, mut rx) = mpsc::unbounded_channel::<CheckReport>();

    // For human mode, stream reports as they complete. For JSON mode, drain the
    // channel silently and print only the final aggregated report.
    let printer = if args.json {
        tokio::spawn(async move { while rx.recv().await.is_some() {} })
    } else {
        tokio::spawn(async move {
            while let Some(report) = rx.recv().await {
                let fixes = if report.fixes_applied.is_empty() {
                    String::new()
                } else {
                    format!(" [fixes: {}]", report.fixes_applied.join(", "))
                };
                println!(
                    "[{:>4}] {:<8} {:<30} {}{}",
                    report.status.label(),
                    tier_label(report.tier),
                    report.name,
                    report.detail,
                    fixes,
                );
            }
        }
        )
    };

    let result = run_doctor(&context, args.fix, tx).await;
    let _ = printer.await;

    let report = result?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "summary: {} ok / {} warn / {} fail",
            report.ok, report.warn, report.fail
        );
    }
    if report.has_failures() {
        return Err(anyhow!("doctor detected failures"));
    }
    Ok(())
}
