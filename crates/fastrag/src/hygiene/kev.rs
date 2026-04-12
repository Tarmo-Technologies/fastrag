//! KevTemporalTagger: tags CVE metadata with `kev_flag=true` when the CVE ID
//! is present in a CISA Known Exploited Vulnerabilities (KEV) catalog.
//!
//! Accepts two catalog shapes detected at load time:
//!   1. CISA `vulnerabilities.json` — `{ "vulnerabilities": [ { "cveID": "CVE-..." }, ... ] }`
//!   2. FastRAG minimal format       — `{ "cve_ids": ["CVE-...", ...] }`
//!
//! The tagger implements `MetadataEnricher` — it only runs on documents that
//! survive the reject + strip + language filters.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

use super::MetadataEnricher;

/// Raw CISA KEV entry (only the fields we need).
#[derive(Debug, Deserialize)]
struct CisaKevEntry {
    #[serde(rename = "cveID")]
    cve_id: String,
}

/// CISA `vulnerabilities.json` top-level shape.
#[derive(Debug, Deserialize)]
struct CisaKevFile {
    vulnerabilities: Vec<CisaKevEntry>,
}

/// FastRAG minimal KEV catalog shape.
#[derive(Debug, Deserialize)]
struct MinimalKevFile {
    cve_ids: Vec<String>,
}

/// Tags chunks whose `cve_id` metadata is in the KEV catalog.
#[derive(Debug)]
pub struct KevTemporalTagger {
    kev_ids: BTreeSet<String>,
}

impl KevTemporalTagger {
    /// Load a KEV catalog from `path`. Accepts CISA `vulnerabilities.json`
    /// shape or the FastRAG minimal `{cve_ids:[...]}` shape, detected at
    /// load time by probing for the `"vulnerabilities"` key.
    pub fn from_path(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path)
            .map_err(|e| format!("cannot read KEV catalog {}: {e}", path.display()))?;
        let raw: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| format!("cannot parse KEV catalog {}: {e}", path.display()))?;

        let kev_ids: BTreeSet<String> = if raw.get("vulnerabilities").is_some() {
            // CISA shape
            let catalog: CisaKevFile = serde_json::from_value(raw)
                .map_err(|e| format!("malformed CISA KEV catalog: {e}"))?;
            catalog
                .vulnerabilities
                .into_iter()
                .map(|e| e.cve_id)
                .collect()
        } else if raw.get("cve_ids").is_some() {
            // FastRAG minimal shape
            let catalog: MinimalKevFile = serde_json::from_value(raw)
                .map_err(|e| format!("malformed minimal KEV catalog: {e}"))?;
            catalog.cve_ids.into_iter().collect()
        } else {
            return Err(format!(
                "unrecognised KEV catalog format in {}; expected 'vulnerabilities' or 'cve_ids' key",
                path.display()
            ));
        };

        Ok(Self { kev_ids })
    }

    /// Build a tagger from an already-loaded ID set (for tests).
    pub fn from_ids(ids: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            kev_ids: ids.into_iter().map(Into::into).collect(),
        }
    }
}

impl MetadataEnricher for KevTemporalTagger {
    fn enrich(&self, metadata: &mut BTreeMap<String, String>) {
        if let Some(cve_id) = metadata.get("cve_id")
            && self.kev_ids.contains(cve_id.as_str())
        {
            metadata.insert("kev_flag".to_string(), "true".to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn meta(cve_id: &str) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("cve_id".to_string(), cve_id.to_string());
        m
    }

    #[test]
    fn tags_known_cve_with_kev_flag() {
        let tagger = KevTemporalTagger::from_ids(["CVE-2021-44228"]);
        let mut m = meta("CVE-2021-44228");
        tagger.enrich(&mut m);
        assert_eq!(m.get("kev_flag").map(String::as_str), Some("true"));
    }

    #[test]
    fn does_not_tag_unknown_cve() {
        let tagger = KevTemporalTagger::from_ids(["CVE-2021-44228"]);
        let mut m = meta("CVE-2024-99999");
        tagger.enrich(&mut m);
        assert!(!m.contains_key("kev_flag"));
    }

    #[test]
    fn does_not_tag_doc_without_cve_id() {
        let tagger = KevTemporalTagger::from_ids(["CVE-2021-44228"]);
        let mut m = BTreeMap::new();
        tagger.enrich(&mut m);
        assert!(!m.contains_key("kev_flag"));
    }

    #[test]
    fn loads_cisa_shape_from_file() {
        let json = r#"{
  "title": "CISA Known Exploited Vulnerabilities Catalog",
  "vulnerabilities": [
    { "cveID": "CVE-2021-44228", "vendorProject": "Apache", "product": "Log4j" },
    { "cveID": "CVE-2022-22965", "vendorProject": "VMware", "product": "Spring Framework" }
  ]
}"#;
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(json.as_bytes()).unwrap();
        let tagger = KevTemporalTagger::from_path(tmp.path()).unwrap();
        let mut m = meta("CVE-2021-44228");
        tagger.enrich(&mut m);
        assert_eq!(m.get("kev_flag").map(String::as_str), Some("true"));
    }

    #[test]
    fn loads_minimal_shape_from_file() {
        let json = r#"{ "cve_ids": ["CVE-2021-44228", "CVE-2019-0708"] }"#;
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(json.as_bytes()).unwrap();
        let tagger = KevTemporalTagger::from_path(tmp.path()).unwrap();
        let mut m = meta("CVE-2019-0708");
        tagger.enrich(&mut m);
        assert_eq!(m.get("kev_flag").map(String::as_str), Some("true"));
    }

    #[test]
    fn rejects_unknown_catalog_shape() {
        let json = r#"{ "data": [] }"#;
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(json.as_bytes()).unwrap();
        let err = KevTemporalTagger::from_path(tmp.path()).unwrap_err();
        assert!(
            err.contains("unrecognised"),
            "error must name the shape; got: {err}"
        );
    }
}
