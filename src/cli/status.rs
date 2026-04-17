use anyhow::Result;
use clap::Args;

use crate::cli::scope::{resolve_context_name, ContextSelectArgs};

#[derive(Debug, Args)]
pub struct StatusArgs {
    #[command(flatten)]
    pub select: ContextSelectArgs,
}

pub async fn run(args: StatusArgs) -> Result<()> {
    let context = resolve_context_name(&args.select)?;
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

    Ok(())
}
