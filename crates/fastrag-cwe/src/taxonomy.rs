use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TaxonomyError {
    #[error(
        "taxonomy schema {found} is not supported; expected 2. \
         Rebuild with: cargo run -p fastrag-cwe --features compile-tool --bin compile-taxonomy"
    )]
    SchemaMismatch { found: u32 },
    #[error("taxonomy parse error: {0}")]
    Parse(#[from] serde_json::Error),
}

fn default_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Taxonomy {
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    version: String,
    view: String,
    /// Map from CWE id → `[self, descendants...]` (self is first, rest sorted ascending).
    closure: HashMap<u32, Vec<u32>>,
    /// Direct parent edges for each CWE. Empty for root nodes.
    #[serde(default)]
    parents: HashMap<u32, Vec<u32>>,
}

impl Taxonomy {
    /// Parse and validate a taxonomy JSON string. Rejects any payload whose
    /// `schema_version` is not 2 and steers the caller at the rebuild command.
    pub fn from_json(s: &str) -> Result<Self, TaxonomyError> {
        let tax: Taxonomy = serde_json::from_str(s)?;
        if tax.schema_version != 2 {
            return Err(TaxonomyError::SchemaMismatch {
                found: tax.schema_version,
            });
        }
        Ok(tax)
    }

    /// Internal constructor used by the compile tool. Not part of the public
    /// runtime API. Always stamps `schema_version` as 2.
    pub fn from_components(
        version: String,
        view: String,
        closure: HashMap<u32, Vec<u32>>,
        parents: HashMap<u32, Vec<u32>>,
    ) -> Self {
        Self {
            schema_version: 2,
            version,
            view,
            closure,
            parents,
        }
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn view(&self) -> &str {
        &self.view
    }

    /// Return the closure for `cwe`. Falls back to a single-element slice
    /// holding `cwe` when the id is not present in the taxonomy.
    pub fn expand(&self, cwe: u32) -> Vec<u32> {
        match self.closure.get(&cwe) {
            Some(ids) => ids.clone(),
            None => vec![cwe],
        }
    }

    /// Direct parents of `cwe` (empty slice for roots or unknown ids).
    pub fn parents(&self, cwe: u32) -> &[u32] {
        self.parents.get(&cwe).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// All ancestors of `cwe` in BFS order with cycle-safe dedupe. A node
    /// reachable via multiple paths appears exactly once.
    pub fn ancestors(&self, cwe: u32) -> Vec<u32> {
        self.ancestors_bounded(cwe, usize::MAX)
    }

    /// Ancestors bounded to at most `max_depth` parent hops. `max_depth == 0`
    /// returns an empty vector; `max_depth == 1` returns direct parents only.
    pub fn ancestors_bounded(&self, cwe: u32, max_depth: usize) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut visited: HashSet<u32> = HashSet::new();
        visited.insert(cwe);
        let mut queue: VecDeque<(u32, usize)> = VecDeque::new();
        queue.push_back((cwe, 0));
        while let Some((node, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            if let Some(parents) = self.parents.get(&node) {
                for &p in parents {
                    if visited.insert(p) {
                        out.push(p);
                        queue.push_back((p, depth + 1));
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_json() -> &'static str {
        r#"{
            "schema_version": 2,
            "version": "4.16-test",
            "view": "1000",
            "closure": {
                "89": [89, 564, 943],
                "79": [79, 80, 81]
            },
            "parents": {}
        }"#
    }

    #[test]
    fn parses_version_and_view() {
        let tx = Taxonomy::from_json(fixture_json()).unwrap();
        assert_eq!(tx.version(), "4.16-test");
        assert_eq!(tx.view(), "1000");
    }

    #[test]
    fn expand_known_id_returns_closure() {
        let tx = Taxonomy::from_json(fixture_json()).unwrap();
        let got = tx.expand(89);
        assert!(got.contains(&89), "expand(89) missing self: {got:?}");
        assert!(got.contains(&564), "expand(89) missing child 564: {got:?}");
        assert!(got.contains(&943), "expand(89) missing child 943: {got:?}");
    }

    #[test]
    fn expand_unknown_id_returns_singleton() {
        let tx = Taxonomy::from_json(fixture_json()).unwrap();
        assert_eq!(tx.expand(9999), vec![9999]);
    }

    #[test]
    fn expand_is_idempotent_on_repeat_calls() {
        let tx = Taxonomy::from_json(fixture_json()).unwrap();
        let first = tx.expand(79);
        let second = tx.expand(79);
        assert_eq!(first, second);
    }

    #[test]
    fn malformed_json_errors() {
        let result = Taxonomy::from_json("not json");
        assert!(matches!(result, Err(TaxonomyError::Parse(_))));
    }

    #[test]
    fn parents_returns_direct_parents_only() {
        // CWE graph: 20 -> 707 -> 89 and 20 -> 943 -> 89 (89 has two parents)
        let json = r#"{
            "schema_version": 2,
            "version": "4.15",
            "view": "1000",
            "closure": {"20": [20, 707, 943, 89], "707": [707, 89], "943": [943, 89], "89": [89]},
            "parents": {"89": [707, 943], "707": [20], "943": [20], "20": []}
        }"#;
        let tax: Taxonomy = serde_json::from_str(json).unwrap();
        let p = tax.parents(89);
        let mut got = p.to_vec();
        got.sort();
        assert_eq!(got, vec![707, 943]);
        assert_eq!(tax.parents(20), &[] as &[u32]);
    }

    #[test]
    fn ancestors_bfs_dedupes_multi_parent_dag() {
        let json = r#"{
            "schema_version": 2, "version": "4.15", "view": "1000",
            "closure": {},
            "parents": {
                "89": [707, 943],
                "707": [20],
                "943": [20],
                "20": []
            }
        }"#;
        let tax: Taxonomy = serde_json::from_str(json).unwrap();
        let a = tax.ancestors(89);
        // BFS order: direct parents first (707, 943 in any order), then 20 once.
        assert_eq!(a.len(), 3);
        assert!(a.contains(&707));
        assert!(a.contains(&943));
        assert!(a.contains(&20));
        // CWE 20 appears only once despite two parent paths.
        assert_eq!(a.iter().filter(|&&x| x == 20).count(), 1);
    }

    #[test]
    fn ancestors_of_root_is_empty() {
        let json = r#"{"schema_version": 2, "version": "4.15", "view": "1000",
                       "closure": {}, "parents": {"20": []}}"#;
        let tax: Taxonomy = serde_json::from_str(json).unwrap();
        assert!(tax.ancestors(20).is_empty());
    }

    #[test]
    fn ancestors_of_unknown_is_empty() {
        let json = r#"{"schema_version": 2, "version": "4.15", "view": "1000",
                       "closure": {}, "parents": {}}"#;
        let tax: Taxonomy = serde_json::from_str(json).unwrap();
        assert!(tax.ancestors(9999).is_empty());
    }

    #[test]
    fn ancestors_bounded_respects_max_depth() {
        let json = r#"{"schema_version": 2, "version": "4.15", "view": "1000",
            "closure": {},
            "parents": {"89": [707], "707": [20], "20": []}}"#;
        let tax: Taxonomy = serde_json::from_str(json).unwrap();
        assert_eq!(tax.ancestors_bounded(89, 0), Vec::<u32>::new());
        assert_eq!(tax.ancestors_bounded(89, 1), vec![707]);
        let mut two = tax.ancestors_bounded(89, 2);
        two.sort();
        assert_eq!(two, vec![20, 707]);
    }

    #[test]
    fn schema_v1_is_rejected_at_load() {
        // v1 payload lacks `parents` and uses `schema_version` 1 (or missing).
        let v1 = r#"{"version":"4.15","view":"1000",
                     "closure":{"20":[20]}}"#;
        let err = Taxonomy::from_json(v1).expect_err("should reject v1");
        let msg = err.to_string();
        assert!(
            msg.contains("schema") || msg.contains("version"),
            "expected schema error, got: {msg}"
        );
        assert!(
            msg.contains("compile-taxonomy") || msg.contains("rebuild"),
            "error should point at the rebuild command, got: {msg}"
        );
    }
}

#[cfg(all(test, feature = "compile-tool"))]
mod compile_tests {
    use crate::compile::{compute_parents, detect_cycles};
    use std::collections::HashMap;

    #[test]
    fn compute_parents_inverts_child_parent_relation() {
        // parents map input: child -> Vec<parent>. Output keyed by node -> direct parents.
        let mut input: HashMap<u32, Vec<u32>> = HashMap::new();
        input.insert(89, vec![707, 943]);
        input.insert(707, vec![20]);
        input.insert(943, vec![20]);
        input.insert(20, vec![]);
        let out = compute_parents(&input);
        let mut p89 = out.get(&89).cloned().unwrap();
        p89.sort();
        assert_eq!(p89, vec![707, 943]);
        assert_eq!(out.get(&20).cloned().unwrap_or_default(), Vec::<u32>::new());
    }

    #[test]
    fn compute_parents_detects_cycle() {
        let mut input: HashMap<u32, Vec<u32>> = HashMap::new();
        input.insert(1, vec![2]);
        input.insert(2, vec![3]);
        input.insert(3, vec![1]);
        let err = detect_cycles(&input).expect_err("cycle expected");
        assert!(err.to_string().to_lowercase().contains("cycle"));
    }
}
