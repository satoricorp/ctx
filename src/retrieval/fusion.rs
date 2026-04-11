use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub struct CCTMMFusion {
    pub alpha: f32,
}

impl Default for CCTMMFusion {
    fn default() -> Self {
        Self { alpha: 0.7 }
    }
}

impl CCTMMFusion {
    fn norm_cosine(score: f32) -> f32 {
        ((score + 1.0) / 2.0).clamp(0.0, 1.0)
    }

    fn norm_bm25(score: f32) -> f32 {
        (score / 2.2).clamp(0.0, 1.0)
    }

    pub fn fuse(&self, dense: Vec<(String, f32)>, sparse: Vec<(String, f32)>) -> Vec<String> {
        let mut scores: HashMap<String, f32> = HashMap::new();
        for (id, score) in &dense {
            *scores.entry(id.clone()).or_default() += self.alpha * Self::norm_cosine(*score);
        }
        for (id, score) in &sparse {
            *scores.entry(id.clone()).or_default() += (1.0 - self.alpha) * Self::norm_bm25(*score);
        }

        let mut merged: Vec<_> = scores.into_iter().collect();
        merged.sort_by(|left, right| right.1.total_cmp(&left.1));
        merged.into_iter().map(|(id, _)| id).collect()
    }
}

