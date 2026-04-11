use anyhow::Result;
use std::path::Path;

use crate::store::schema::AddOutcome;

pub async fn ingest_semantic_path(_ctx_path: &Path, _source_path: &Path) -> Result<AddOutcome> {
    anyhow::bail!("semantic indexing is not implemented yet")
}

