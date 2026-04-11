use anyhow::Result;
use std::path::Path;

use crate::store::schema::AddOutcome;

pub async fn ingest_procedural_path(_ctx_path: &Path, _source_path: &Path) -> Result<AddOutcome> {
    anyhow::bail!("procedural indexing is not implemented yet")
}

