use std::collections::{HashMap, HashSet};

fn consider_top_k(retrieved: &[String], k: usize) -> impl Iterator<Item = &String> {
    retrieved.iter().take(k)
}

pub fn recall_at_k(retrieved: &[String], relevant: &HashSet<String>, k: usize) -> f64 {
    if relevant.is_empty() || k == 0 {
        return 0.0;
    }
    let mut seen = HashSet::new();
    let hits = consider_top_k(retrieved, k)
        .filter(|doc_id| seen.insert((*doc_id).clone()))
        .filter(|doc_id| relevant.contains(*doc_id))
        .count();
    hits as f64 / relevant.len() as f64
}

pub fn mrr_at_k(retrieved: &[String], relevant: &HashSet<String>, k: usize) -> f64 {
    if relevant.is_empty() || k == 0 {
        return 0.0;
    }
    let mut seen = HashSet::new();
    for (idx, doc_id) in consider_top_k(retrieved, k).enumerate() {
        if !seen.insert(doc_id.clone()) {
            continue;
        }
        if relevant.contains(doc_id) {
            return 1.0 / (idx + 1) as f64;
        }
    }
    0.0
}

pub fn ndcg_at_k(retrieved: &[String], qrels: &HashMap<String, u32>, k: usize) -> f64 {
    if k == 0 || qrels.is_empty() {
        return 0.0;
    }

    let dcg = dcg(retrieved, qrels, k);
    let mut ideal_rels = qrels.values().copied().collect::<Vec<_>>();
    ideal_rels.sort_unstable_by(|a, b| b.cmp(a));
    let idcg = dcg_from_rels(&ideal_rels, k);

    if idcg == 0.0 { 0.0 } else { dcg / idcg }
}

pub fn hit_rate_at_k(retrieved: &[String], relevant: &HashSet<String>, k: usize) -> f64 {
    if relevant.is_empty() || k == 0 {
        return 0.0;
    }
    let mut seen = HashSet::new();
    if consider_top_k(retrieved, k)
        .any(|doc_id| seen.insert(doc_id.clone()) && relevant.contains(doc_id))
    {
        1.0
    } else {
        0.0
    }
}

fn dcg(retrieved: &[String], qrels: &HashMap<String, u32>, k: usize) -> f64 {
    let mut seen = HashSet::new();
    let mut score = 0.0;
    for (idx, doc_id) in consider_top_k(retrieved, k).enumerate() {
        if !seen.insert(doc_id.clone()) {
            continue;
        }
        let relevance = qrels.get(doc_id).copied().unwrap_or(0);
        score += gain(relevance) / log2(idx + 2);
    }
    score
}

fn dcg_from_rels(relevances: &[u32], k: usize) -> f64 {
    relevances
        .iter()
        .copied()
        .take(k)
        .enumerate()
        .map(|(idx, relevance)| gain(relevance) / log2(idx + 2))
        .sum()
}

fn gain(relevance: u32) -> f64 {
    (2f64.powi(relevance as i32) - 1.0).max(0.0)
}

fn log2(value: usize) -> f64 {
    (value as f64).log2()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(values: &[&str]) -> HashSet<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn recall_at_k_hand_computed() {
        let retrieved = vec!["d3", "d1", "d2", "d5"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let relevant = set(&["d1", "d2", "d4"]);

        assert!((recall_at_k(&retrieved, &relevant, 1) - 0.0).abs() < 1e-12);
        assert!((recall_at_k(&retrieved, &relevant, 2) - (1.0 / 3.0)).abs() < 1e-12);
        assert!((recall_at_k(&retrieved, &relevant, 3) - (2.0 / 3.0)).abs() < 1e-12);
        assert!((recall_at_k(&retrieved, &relevant, 4) - (2.0 / 3.0)).abs() < 1e-12);
    }

    #[test]
    fn mrr_hand_computed() {
        let retrieved = vec!["d3", "d1", "d2"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let relevant = set(&["d1", "d2"]);
        assert!((mrr_at_k(&retrieved, &relevant, 1) - 0.0).abs() < 1e-12);
        assert!((mrr_at_k(&retrieved, &relevant, 3) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn ndcg_hand_computed() {
        let retrieved = vec!["d2", "d1", "d3"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let qrels = HashMap::from([
            ("d1".to_string(), 3),
            ("d2".to_string(), 2),
            ("d3".to_string(), 1),
        ]);

        let score = ndcg_at_k(&retrieved, &qrels, 3);
        let expected =
            (3.0 + (7.0 / 1.584962500721156) + 0.5) / (7.0 + (3.0 / 1.584962500721156) + 0.5);
        assert!((score - expected).abs() < 1e-12);

        let perfect = vec!["d1", "d2", "d3"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        assert!((ndcg_at_k(&perfect, &qrels, 3) - 1.0).abs() < 1e-12);

        let worst = vec!["x", "y", "d3"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        assert!(ndcg_at_k(&worst, &qrels, 3) < 0.3);
    }

    #[test]
    fn hit_rate_at_k_hand_computed() {
        let retrieved = vec!["d3", "d1", "d2"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let relevant = set(&["d1", "d2"]);
        assert!((hit_rate_at_k(&retrieved, &relevant, 1) - 0.0).abs() < 1e-12);
        assert!((hit_rate_at_k(&retrieved, &relevant, 2) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn recall_at_k_handles_k_larger_than_results() {
        let retrieved = vec!["d1", "d3"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let relevant = set(&["d1", "d2"]);
        assert!((recall_at_k(&retrieved, &relevant, 10) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn ndcg_handles_no_relevant_docs() {
        let retrieved = vec!["d1", "d2"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let qrels = HashMap::new();
        assert_eq!(ndcg_at_k(&retrieved, &qrels, 10), 0.0);
    }
}
