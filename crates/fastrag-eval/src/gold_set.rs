//! Gold set schema loader + union-of-top-k scorer.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GoldSet {
    pub version: u32,
    pub entries: Vec<GoldSetEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GoldSetEntry {
    pub id: String,
    pub question: String,
    #[serde(default)]
    pub must_contain_cve_ids: Vec<String>,
    #[serde(default)]
    pub must_contain_terms: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gold_set_round_trips_through_json() {
        let gs = GoldSet {
            version: 1,
            entries: vec![GoldSetEntry {
                id: "q001".into(),
                question: "Is there an RCE in libfoo?".into(),
                must_contain_cve_ids: vec!["CVE-2024-12345".into()],
                must_contain_terms: vec!["libfoo".into()],
                notes: None,
            }],
        };
        let json = serde_json::to_string(&gs).unwrap();
        let back: GoldSet = serde_json::from_str(&json).unwrap();
        assert_eq!(gs, back);
    }
}
