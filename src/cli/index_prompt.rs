//! Interactive sync vs background choice for long indexing runs.

use anyhow::Result;
use dialoguer::Select;
use std::io::{self, IsTerminal};

use crate::index_plan::{self, WorkPlan};

const PROMPT_FILE_THRESHOLD: usize = 10;
const PROMPT_SECONDS_THRESHOLD: f64 = 60.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexRunChoice {
    Sync,
    Background,
    Cancel,
}

pub fn should_offer_choice(
    plan: &WorkPlan,
    est_secs: f64,
    yes: bool,
    background: bool,
    dry_run: bool,
) -> bool {
    !dry_run
        && !yes
        && !background
        && !plan.is_empty()
        && io::stdin().is_terminal()
        && (plan.items.len() >= PROMPT_FILE_THRESHOLD || est_secs >= PROMPT_SECONDS_THRESHOLD)
}

pub fn prompt_run_mode(plan: &WorkPlan, est_secs: f64) -> Result<IndexRunChoice> {
    let est = index_plan::format_duration_humans(est_secs);
    eprintln!("{}", index_plan::describe_plan_line(plan));
    eprintln!(
        "rough time estimate: {est} (network and model dependent; not a guarantee)\n\
         macOS: system sleep can pause or break long runs; consider `caffeinate -dims ctx ...` for all-night indexing."
    );

    let items = vec![
        "Run now (sync, this terminal)",
        "Run in background (detached; use `ctx status` for progress)",
        "Cancel",
    ];
    let i = Select::new()
        .with_prompt("How should ctx run this index job?")
        .items(&items)
        .default(0)
        .interact()?;

    Ok(match i {
        0 => IndexRunChoice::Sync,
        1 => IndexRunChoice::Background,
        _ => IndexRunChoice::Cancel,
    })
}
