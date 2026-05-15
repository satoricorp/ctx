use anyhow::{anyhow, Result};
use clap::Args;
use tokio::sync::mpsc;

use crate::cli::progress::CliSpinner;
use crate::cli::scope::{resolve_context_name, ContextSelectArgs};
use crate::cli::theme;
use crate::doctor::{run_doctor, CheckReport, CheckStatus, DoctorReport};

const COLOR_OK: &str = "\x1b[32;1m";
const COLOR_WARN: &str = "\x1b[33;1m";
const COLOR_FAIL: &str = "\x1b[31;1m";

#[derive(Debug, Args)]
#[command(about = "Check a context for problems and optionally repair them")]
pub struct DoctorArgs {
    #[command(flatten)]
    pub select: ContextSelectArgs,
    /// Apply non-destructive repairs (prune orphan blobs, sync notes registry,
    /// relocate stray root-level topics, rebuild the index if needed).
    #[arg(long = "fix")]
    pub fix: bool,
    /// Emit a machine-readable JSON report.
    #[arg(long = "json")]
    pub json: bool,
}

pub async fn run(args: DoctorArgs) -> Result<()> {
    let context = resolve_context_name(&args.select)?;

    let (tx, mut rx) = mpsc::unbounded_channel::<CheckReport>();

    // Drain streamed updates silently; human mode renders the final sorted report once complete.
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });

    let spinner = if args.json {
        None
    } else {
        Some(CliSpinner::new(if args.fix {
            "running doctor and applying fixes"
        } else {
            "running doctor"
        }))
    };
    let result = run_doctor(&context, args.fix, tx).await;
    let _ = drain.await;

    let report = result?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        if let Some(spinner) = spinner {
            spinner.success("doctor complete");
        }
        print_human_report(&report);
    }
    if report.has_failures() {
        return Err(anyhow!("doctor detected failures"));
    }
    Ok(())
}

fn print_human_report(report: &DoctorReport) {
    println!(
        "{} {}",
        theme::section("Doctor"),
        theme::command(&report.context)
    );
    for item in &report.reports {
        print_report_line(item);
    }
    println!();
    println!(
        "{} {}{}{} ok  {}{}{} warn  {}{}{} fail",
        theme::muted("summary:"),
        COLOR_OK,
        report.ok,
        theme::RESET,
        COLOR_WARN,
        report.warn,
        theme::RESET,
        COLOR_FAIL,
        report.fail,
        theme::RESET
    );
}

fn print_report_line(report: &CheckReport) {
    let (status_label, status_color) = match report.status {
        CheckStatus::Ok => ("OK", COLOR_OK),
        CheckStatus::Warn => ("WARN", COLOR_WARN),
        CheckStatus::Fail => ("FAIL", COLOR_FAIL),
    };
    println!(
        "{status_color}{:>4}{}  {}  {}",
        status_label,
        theme::RESET,
        theme::command(display_name(report.name)),
        report.detail,
    );
    if !report.fixes_applied.is_empty() {
        println!(
            "      {}{}fix:{} {}",
            theme::GREEN,
            theme::BOLD,
            theme::RESET,
            report.fixes_applied.join(", ")
        );
    }
}

fn display_name(name: &str) -> &'static str {
    match name {
        "manifest_schema" => "Manifest",
        "config_sanity" => "Config",
        "notes_registry_missing" => "Missing Notes",
        "notes_registry_unregistered" => "Notes Registry",
        "index_presence" => "Index",
        "source_drift" => "Source Drift",
        "blob_integrity" => "Blob Integrity",
        "index_rebuild" => "Index Rebuild",
        _ => "Check",
    }
}
