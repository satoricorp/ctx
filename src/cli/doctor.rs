use anyhow::{anyhow, Result};
use clap::Args;
use tokio::sync::mpsc;

use crate::cli::scope::{apply_context_image_flag, ContextSelectArgs};
use crate::doctor::{run_doctor, tier_label, CheckReport};

#[derive(Debug, Args)]
pub struct DoctorArgs {
    #[command(flatten)]
    pub select: ContextSelectArgs,
    /// Apply non-destructive repairs (prune orphan blobs, sync aura registry,
    /// relocate stray root-level topics, rebuild the index if needed).
    #[arg(long = "fix")]
    pub fix: bool,
}

pub async fn run(args: DoctorArgs) -> Result<()> {
    apply_context_image_flag(&args.select.image);
    let context = args
        .select
        .context
        .clone()
        .unwrap_or(crate::artifact::infer_context_name()?);

    let (tx, mut rx) = mpsc::unbounded_channel::<CheckReport>();

    // Stream reports as they arrive. Spawn the printer so doctor can send freely.
    let printer = tokio::spawn(async move {
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
    });

    let result = run_doctor(&context, args.fix, tx).await;
    let _ = printer.await;

    let report = result?;
    println!(
        "summary: {} ok / {} warn / {} fail",
        report.ok, report.warn, report.fail
    );
    if report.has_failures() {
        return Err(anyhow!("doctor detected failures"));
    }
    Ok(())
}
