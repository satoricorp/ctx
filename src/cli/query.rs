use anyhow::Result;
use clap::Args;

use crate::retrieval::query::QueryType;

#[derive(Debug, Args)]
pub struct QueryArgs {
    #[arg(value_name = "text")]
    pub query: String,
    #[arg(short = 'c', long = "context")]
    pub context: Option<String>,
    #[arg(long = "type", default_value_t = QueryType::All)]
    pub kind: QueryType,
    #[arg(long = "k", default_value_t = 5)]
    pub k: usize,
}

pub async fn run(args: QueryArgs) -> Result<()> {
    let context = args
        .context
        .unwrap_or(crate::artifact::infer_context_name()?);
    let results = crate::query_context(&context, &args.query, args.kind, args.k).await?;
    if results.is_empty() {
        println!("no results");
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
