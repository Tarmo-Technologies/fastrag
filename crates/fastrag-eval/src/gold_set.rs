//! Gold set schema loader + union-of-top-k scorer.

use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::EvalError;

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

static CVE_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^CVE-\d{4}-\d+$").unwrap());

pub fn load(path: &Path) -> Result<GoldSet, EvalError> {
    let bytes = std::fs::read(path).map_err(EvalError::from)?;
    let gs: GoldSet = serde_json::from_slice(&bytes).map_err(|e| EvalError::GoldSetParse {
        path: path.to_path_buf(),
        source: e,
    })?;
    validate(&gs)?;
    Ok(gs)
}

fn validate(gs: &GoldSet) -> Result<(), EvalError> {
    if gs.version == 0 {
        return Err(EvalError::GoldSetInvalid("version must be >= 1".into()));
    }
    let mut seen: HashSet<&str> = HashSet::new();
    for entry in &gs.entries {
        if entry.id.is_empty() {
            return Err(EvalError::GoldSetInvalid(
                "entry with empty id is not allowed".into(),
            ));
        }
        if !seen.insert(entry.id.as_str()) {
            return Err(EvalError::GoldSetInvalid(format!(
                "duplicate entry id '{}'",
                entry.id
            )));
        }
        if entry.question.trim().is_empty() {
            return Err(EvalError::GoldSetInvalid(format!(
                "entry '{}' has empty question",
                entry.id
            )));
        }
        if entry.must_contain_cve_ids.is_empty() && entry.must_contain_terms.is_empty() {
            return Err(EvalError::GoldSetInvalid(format!(
                "entry '{}' has no must_contain_cve_ids and no must_contain_terms",
                entry.id
            )));
        }
        for cve in &entry.must_contain_cve_ids {
            if !CVE_ID_RE.is_match(cve) {
                return Err(EvalError::GoldSetInvalid(format!(
                    "entry '{}' must_contain_cve_ids contains malformed id '{}'",
                    entry.id, cve
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_fixture(json: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(json.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

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

    #[test]
    fn load_accepts_well_formed_gold_set() {
        let f = write_fixture(r#"{
            "version": 1,
            "entries": [
                {"id": "q001", "question": "x?", "must_contain_cve_ids": ["CVE-2024-1"], "must_contain_terms": []}
            ]
        }"#);
        let gs = load(f.path()).expect("valid gold set should load");
        assert_eq!(gs.entries.len(), 1);
        assert_eq!(gs.entries[0].id, "q001");
    }

    #[test]
    fn load_rejects_empty_question() {
        let f = write_fixture(r#"{
            "version": 1,
            "entries": [
                {"id": "q001", "question": "", "must_contain_cve_ids": ["CVE-2024-1"], "must_contain_terms": []}
            ]
        }"#);
        let err = load(f.path()).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("q001"), "error must name offending id, got: {msg}");
        assert!(msg.contains("empty question"), "error must say 'empty question', got: {msg}");
    }

    #[test]
    fn load_rejects_duplicate_id() {
        let f = write_fixture(r#"{
            "version": 1,
            "entries": [
                {"id": "q001", "question": "a?", "must_contain_cve_ids": ["CVE-2024-1"], "must_contain_terms": []},
                {"id": "q001", "question": "b?", "must_contain_cve_ids": ["CVE-2024-2"], "must_contain_terms": []}
            ]
        }"#);
        let err = load(f.path()).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("duplicate"), "got: {msg}");
        assert!(msg.contains("q001"), "got: {msg}");
    }

    #[test]
    fn load_rejects_malformed_cve_id() {
        let f = write_fixture(r#"{
            "version": 1,
            "entries": [
                {"id": "q001", "question": "x?", "must_contain_cve_ids": ["CVE-24-1"], "must_contain_terms": []}
            ]
        }"#);
        let err = load(f.path()).unwrap_err();
        assert!(format!("{err}").contains("CVE-24-1"));
    }

    #[test]
    fn load_rejects_zero_assertions() {
        let f = write_fixture(r#"{
            "version": 1,
            "entries": [
                {"id": "q001", "question": "x?", "must_contain_cve_ids": [], "must_contain_terms": []}
            ]
        }"#);
        let err = load(f.path()).unwrap_err();
        assert!(format!("{err}").contains("no must_contain"));
    }
}
