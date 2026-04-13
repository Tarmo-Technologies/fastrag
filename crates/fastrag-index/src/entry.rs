use serde::{Deserialize, Serialize};

/// Slim vector-only entry. All text, metadata, and _source live in Tantivy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorEntry {
    pub id: u64,
    pub vector: Vec<f32>,
}

/// Dense search result: vector entry + cosine similarity score.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorHit {
    pub id: u64,
    pub score: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_entry_serde_round_trip() {
        let entry = VectorEntry {
            id: 42,
            vector: vec![0.1, 0.2, 0.3],
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let parsed: VectorEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(entry, parsed);
    }
}
