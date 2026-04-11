use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::Args;

#[derive(Debug, Args)]
pub struct ListArgs;

pub async fn run(_args: ListArgs) -> Result<()> {
    for context in crate::list_contexts()? {
        println!(
            "{} {} chunks {} entities {}",
            context.name,
            context.counts.chunk_count,
            context.counts.entity_count,
            humanize_age(context.updated_at),
        );
    }
    Ok(())
}

fn humanize_age(timestamp: Option<DateTime<Utc>>) -> String {
    let Some(timestamp) = timestamp else {
        return String::from("unknown");
    };

    let delta = Utc::now() - timestamp;
    if delta.num_days() > 0 {
        return format!("{}d ago", delta.num_days());
    }
    if delta.num_hours() > 0 {
        return format!("{}h ago", delta.num_hours());
    }
    if delta.num_minutes() > 0 {
        return format!("{}m ago", delta.num_minutes());
    }
    String::from("just now")
}
