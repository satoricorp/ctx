use anyhow::Result;

pub async fn rerank_documents(_query: &str, _documents: &[String]) -> Result<Vec<(usize, f32)>> {
    anyhow::bail!("reranking is not implemented yet")
}

