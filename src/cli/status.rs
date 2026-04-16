use anyhow::{anyhow, Result};
use clap::Args;

use crate::cli::scope::{apply_context_image_flag, ContextSelectArgs};
use crate::IntegrityStatus;

#[derive(Debug, Args)]
pub struct StatusArgs {
    #[command(flatten)]
    pub select: ContextSelectArgs,
    /// Verify blob integrity and surface orphan blobs (spec §13.1, §13.5).
    #[arg(long = "verify")]
    pub verify: bool,
}

pub async fn run(args: StatusArgs) -> Result<()> {
    apply_context_image_flag(&args.select.image);
    let context = args
        .select
        .context
        .clone()
        .unwrap_or(crate::artifact::infer_context_name()?);
    let status = crate::context_status(&context)?;
    println!("context {}", status.name);
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
    if !status.drifted_files.is_empty() {
        println!("drifted:");
        for path in &status.drifted_files {
            println!("  {}", path);
        }
        println!("hint: run `ctx update` to re-index");
    }

    if args.verify {
        let report = crate::verify_context(&context)?;
        if report.entries.is_empty() {
            println!("verify: no blob-backed entries");
        } else {
            println!("verify:");
            for entry in &report.entries {
                let label = match entry.status {
                    IntegrityStatus::Ok => "ok",
                    IntegrityStatus::Tampered => "tampered",
                    IntegrityStatus::Missing => "missing",
                };
                match &entry.reason {
                    Some(reason) => println!(
                        "  {:8} {}  {}  {}",
                        label, entry.path, entry.blob_ref, reason
                    ),
                    None => println!("  {:8} {}  {}", label, entry.path, entry.blob_ref),
                }
            }
        }
        if !report.orphans.is_empty() {
            println!("orphan blobs:");
            for orphan in &report.orphans {
                println!("  {}", orphan.blob_hash);
            }
        }
        if report.has_failures {
            return Err(anyhow!("integrity check failed"));
        }
    }
    Ok(())
}
