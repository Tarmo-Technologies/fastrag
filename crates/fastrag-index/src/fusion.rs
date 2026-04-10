//! Reciprocal Rank Fusion — merges multiple ranked result lists into a single
//! ranking by accumulating `1 / (k + rank + 1)` per item across all lists.

/// A document ID with a fusion score.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredId {
    pub id: u64,
    pub score: f32,
}

/// Fuse multiple ranked lists using Reciprocal Rank Fusion.
///
/// For each item at 0-indexed rank `r` in each list, accumulates
/// `1.0 / (k + r + 1)` into a per-ID score. Returns results sorted by
/// descending fused score.
pub fn rrf_fuse(lists: &[&[ScoredId]], k: u32) -> Vec<ScoredId> {
    use std::collections::HashMap;

    let mut scores: HashMap<u64, f32> = HashMap::new();

    for list in lists {
        for (rank, item) in list.iter().enumerate() {
            *scores.entry(item.id).or_default() += 1.0 / (k as f32 + rank as f32 + 1.0);
        }
    }

    let mut result: Vec<ScoredId> = scores
        .into_iter()
        .map(|(id, score)| ScoredId { id, score })
        .collect();

    result.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sid(id: u64, score: f32) -> ScoredId {
        ScoredId { id, score }
    }

    #[test]
    fn empty_lists_produce_empty_output() {
        let result = rrf_fuse(&[], 60);
        assert!(result.is_empty());

        let empty: &[ScoredId] = &[];
        let result = rrf_fuse(&[empty], 60);
        assert!(result.is_empty());
    }

    #[test]
    fn single_list_preserves_ranking() {
        let list = vec![sid(1, 0.9), sid(2, 0.8), sid(3, 0.7)];
        let result = rrf_fuse(&[&list], 60);

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].id, 1);
        assert_eq!(result[1].id, 2);
        assert_eq!(result[2].id, 3);
    }

    #[test]
    fn disjoint_lists_produce_union() {
        let a = vec![sid(1, 0.9), sid(2, 0.8)];
        let b = vec![sid(3, 0.7), sid(4, 0.6)];
        let result = rrf_fuse(&[&a, &b], 60);

        assert_eq!(result.len(), 4);
        // rank-0 items from both lists should tie (same score 1/61)
        let ids: Vec<u64> = result.iter().map(|s| s.id).collect();
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
        assert!(ids.contains(&3));
        assert!(ids.contains(&4));
    }

    #[test]
    fn overlapping_items_get_boosted() {
        // Item 1 appears at rank 0 in both lists → double score
        // Item 2 appears at rank 1 in list a only
        // Item 3 appears at rank 0 in list b only (but only once)
        let a = vec![sid(1, 0.9), sid(2, 0.8)];
        let b = vec![sid(1, 0.7), sid(3, 0.6)];
        let result = rrf_fuse(&[&a, &b], 60);

        assert_eq!(result[0].id, 1, "overlapping item should be ranked first");
        // Item 1 score = 1/61 + 1/61 = 2/61
        let expected_score = 2.0 / 61.0;
        assert!(
            (result[0].score - expected_score).abs() < 1e-6,
            "expected {expected_score}, got {}",
            result[0].score
        );
    }

    #[test]
    fn k_parameter_affects_score_spread() {
        let list = vec![sid(1, 0.9), sid(2, 0.8)];

        let result_k1 = rrf_fuse(&[&list], 1);
        let result_k100 = rrf_fuse(&[&list], 100);

        // With k=1: scores are 1/2 and 1/3, spread = 1/6
        let spread_k1 = result_k1[0].score - result_k1[1].score;
        // With k=100: scores are 1/101 and 1/102, spread ≈ 0.0001
        let spread_k100 = result_k100[0].score - result_k100[1].score;

        assert!(
            spread_k1 > spread_k100,
            "smaller k should produce larger score spread"
        );
    }

    #[test]
    fn rrf_scores_follow_formula() {
        let a = vec![sid(10, 1.0), sid(20, 0.5)];
        let b = vec![sid(20, 0.9), sid(10, 0.4)];
        let result = rrf_fuse(&[&a, &b], 60);

        // id=10: rank 0 in a (1/61) + rank 1 in b (1/62) = 0.01639 + 0.01613 = 0.03252
        // id=20: rank 1 in a (1/62) + rank 0 in b (1/61) = 0.01613 + 0.01639 = 0.03252
        // Both should have equal scores (symmetric)
        assert!(
            (result[0].score - result[1].score).abs() < 1e-6,
            "symmetric items should have equal fused scores"
        );
    }
}
