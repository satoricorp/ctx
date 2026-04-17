use anyhow::Result;
use clap::Args;

use crate::cli::scope::{resolve_context_name, ContextSelectArgs};
use crate::retrieval::query::QueryType;

#[derive(Debug, Args)]
pub struct QueryArgs {
    #[command(flatten)]
    pub select: ContextSelectArgs,
    #[arg(value_name = "text")]
    pub query: String,
    #[arg(long = "type", default_value_t = QueryType::All)]
    pub kind: QueryType,
    #[arg(long = "k", default_value_t = 5)]
    pub k: usize,
}

pub async fn run(args: QueryArgs) -> Result<()> {
    let context = resolve_context_name(&args.select)?;
    let results = crate::query_context(&context, &args.query, args.kind, args.k).await?;
    if results.is_empty() {
        let status = crate::context_status(&context)?;
        eprintln!(
            "no results for context {:?} (chunks {}, procedures {})",
            context, status.counts.chunk_count, status.counts.procedure_count
        );
        return Ok(());
    }

    for result in results {
        println!("{} {:.3} {}", result.kind, result.score, result.source);
        println!("{}", result.summary);
        println!("{}", result.content);
        println!();
    }
    Ok(())
}
