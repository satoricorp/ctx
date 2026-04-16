use anyhow::Result;
use clap::Args;

use crate::cli::scope::{apply_context_image_flag, ContextSelectArgs};
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
    apply_context_image_flag(&args.select.image);
    let context = args
        .select
        .context
        .clone()
        .unwrap_or(crate::artifact::infer_context_name()?);
    let results = crate::query_context(&context, &args.query, args.kind, args.k).await?;
    if results.is_empty() {
        let status = crate::context_status(&context)?;
        let image_label = std::env::var("CTX_IMAGE").unwrap_or_else(|_| String::from("(default)"));
        eprintln!(
            "no results for context {:?} (image: {}; chunks {}, procedures {})",
            context,
            image_label,
            status.counts.chunk_count,
            status.counts.procedure_count
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
