use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct StatusArgs {
    #[arg(short = 'c', long = "context")]
    pub context: Option<String>,
}

pub async fn run(args: StatusArgs) -> Result<()> {
    let context = args.context.unwrap_or(crate::artifact::infer_context_name()?);
    let status = crate::context_status(&context)?;
    println!("context {}", status.name);
    println!("indexed {} dirty {} pending {}", status.indexed_count, status.dirty_count, status.pending_count);
    println!(
        "chunks {} entities {} relations {} procedures {}",
        status.counts.chunk_count,
        status.counts.entity_count,
        status.counts.relation_count,
        status.counts.procedure_count,
    );
    println!("extraction {}", status.extraction_model);
    println!("embedding {}", status.embedding_model);
    println!("splade {}", if status.splade_enabled { "enabled" } else { "disabled" });
    Ok(())
}
