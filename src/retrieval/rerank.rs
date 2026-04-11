use anyhow::Result;

use crate::models::embeddings::lexical_overlap;

pub async fn rerank_documents(query: &str, documents: &[String]) -> Result<Vec<(usize, f32)>> {
    let mut scored: Vec<(usize, f32)> = documents
        .iter()
        .enumerate()
        .map(|(index, document)| (index, lexical_overlap(query, document)))
        .collect();

    scored.sort_by(|left, right| right.1.total_cmp(&left.1));
    Ok(scored)
}
