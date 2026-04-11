use anyhow::Result;

pub async fn rerank(_query: &str, _documents: &[String]) -> Result<Vec<(usize, f32)>> {
    anyhow::bail!("reranker is not implemented yet")
}
