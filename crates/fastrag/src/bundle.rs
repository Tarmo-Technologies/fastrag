//! Multi-corpus bundle format for airgap sneakernet delivery.
//!
//! A bundle is a directory tree shipped as a single unit (typically on a DVD
//! ISO or USB stick) that pairs a set of persisted corpora with a compiled
//! CWE taxonomy. [`BundleState`] is the unit of atomic reload: it holds an
//! `Arc<Corpus>` per required corpus plus an `Arc<Taxonomy>`, ready to wrap
//! in `ArcSwap` for lock-free swaps at runtime.
//!
//! Expected on-disk layout:
//! ```text
//! <root>/
//!   bundle.json                 # BundleManifest (schema v1)
//!   corpora/
//!     cve/manifest.json, index.bin, entries.bin
//!     cwe/manifest.json, index.bin, entries.bin
//!     kev/manifest.json, index.bin, entries.bin
//!   taxonomy/
//!     cwe-taxonomy.json          # Taxonomy (schema v2)
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

use fastrag_cwe::Taxonomy;

// Required corpora are declared in bundle.json (manifest.corpora), not fixed at compile time.
const BUNDLE_SCHEMA_VERSION: u32 = 1;

/// Contents of `bundle.json` at the bundle root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleManifest {
    pub schema_version: u32,
    pub bundle_id: String,
    pub built_at: String,
    pub corpora: Vec<String>,
    pub taxonomy: String,
    #[serde(default)]
    pub sources: serde_json::Value,
}

/// Thin handle to a persisted corpus directory. Validates the required
/// files exist at open time without actually loading vectors — those stay
/// behind the existing path-based query functions (`query_corpus*`, etc.),
/// which require an embedder that isn't available at bundle-load time.
#[derive(Debug, Clone)]
pub struct Corpus {
    dir: PathBuf,
}

impl Corpus {
    /// Validate that the corpus directory contains a `manifest.json`,
    /// `index.bin`, and `entries.bin`, then wrap the path.
    pub fn open(dir: &Path) -> Result<Self, BundleError> {
        for required in ["manifest.json", "index.bin", "entries.bin"] {
            let path = dir.join(required);
            if !path.exists() {
                return Err(BundleError::CorpusFileMissing(path));
            }
        }
        Ok(Self {
            dir: dir.to_path_buf(),
        })
    }

    /// Corpus root directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// All errors produced by the bundle loader.
#[derive(Debug, Error)]
pub enum BundleError {
    #[error("bundle schema mismatch: found {found}, expected {BUNDLE_SCHEMA_VERSION}")]
    SchemaMismatch { found: u32 },
    #[error("bundle missing required corpus: {0}")]
    CorpusMissing(String),
    #[error("bundle corpus file missing at {0}")]
    CorpusFileMissing(PathBuf),
    #[error("bundle taxonomy file missing at {0}")]
    TaxonomyMissing(PathBuf),
    #[error("bundle manifest missing at {0}")]
    ManifestMissing(PathBuf),
    #[error("bundle io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("bundle parse error: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("taxonomy load error: {0}")]
    Taxonomy(#[from] fastrag_cwe::TaxonomyError),
}

/// Parse a `bundle.json` blob and reject anything that does not match
/// `BUNDLE_SCHEMA_VERSION`.
pub fn parse_manifest(json: &str) -> Result<BundleManifest, BundleError> {
    let m: BundleManifest = serde_json::from_str(json)?;
    if m.schema_version != BUNDLE_SCHEMA_VERSION {
        return Err(BundleError::SchemaMismatch {
            found: m.schema_version,
        });
    }
    if m.corpora.is_empty() {
        return Err(BundleError::CorpusMissing(
            "<none declared in manifest>".into(),
        ));
    }
    Ok(m)
}

/// Validate the bundle directory tree: `bundle.json`, every required corpus
/// dir (with its own `manifest.json`), and the taxonomy file named by the
/// manifest. Returns the parsed manifest on success.
pub fn validate_layout(root: &Path) -> Result<BundleManifest, BundleError> {
    let manifest_path = root.join("bundle.json");
    if !manifest_path.exists() {
        return Err(BundleError::ManifestMissing(manifest_path));
    }
    let manifest_str = std::fs::read_to_string(&manifest_path)?;
    let manifest = parse_manifest(&manifest_str)?;

    for corpus in &manifest.corpora {
        let corpus: &str = corpus;
        let dir = root.join("corpora").join(corpus);
        if !dir.is_dir() {
            return Err(BundleError::CorpusMissing(corpus.to_string()));
        }
        let mf = dir.join("manifest.json");
        if !mf.exists() {
            return Err(BundleError::CorpusMissing(format!(
                "{corpus} (manifest.json missing)"
            )));
        }
    }

    let taxonomy = root.join("taxonomy").join(&manifest.taxonomy);
    if !taxonomy.exists() {
        return Err(BundleError::TaxonomyMissing(taxonomy));
    }

    Ok(manifest)
}

/// Loaded, validated bundle. Handed to `ArcSwap` in the HTTP server so
/// `/admin/reload` can swap bundles atomically.
#[derive(Debug)]
pub struct BundleState {
    pub corpora: HashMap<String, Arc<Corpus>>,
    pub taxonomy: Arc<Taxonomy>,
    pub manifest: BundleManifest,
}

impl BundleState {
    /// Validate layout, parse the manifest, load the taxonomy, and open
    /// every required corpus directory.
    pub fn load(root: &Path) -> Result<Self, BundleError> {
        let manifest = validate_layout(root)?;

        let tax_path = root.join("taxonomy").join(&manifest.taxonomy);
        let tax_json = std::fs::read_to_string(&tax_path)?;
        let taxonomy = Arc::new(Taxonomy::from_json(&tax_json)?);

        let mut corpora: HashMap<String, Arc<Corpus>> = HashMap::new();
        for name in &manifest.corpora {
            let name: &str = name;
            let dir = root.join("corpora").join(name);
            let corpus = Corpus::open(&dir)
                .map_err(|e| BundleError::CorpusMissing(format!("{name}: {e}")))?;
            corpora.insert(name.to_string(), Arc::new(corpus));
        }

        Ok(BundleState {
            corpora,
            taxonomy,
            manifest,
        })
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::Path;

    /// Write a minimally valid corpus directory: just the three files
    /// [`Corpus::open`] checks for. Contents are placeholders — this helper
    /// is only meant for layout/loader tests that don't exercise the HNSW
    /// index itself.
    pub fn write_empty_corpus(dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join("manifest.json"), "{}")?;
        std::fs::write(dir.join("index.bin"), b"")?;
        std::fs::write(dir.join("entries.bin"), b"")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_manifest_happy_path() {
        let json = r#"{
            "schema_version": 1,
            "bundle_id": "fastrag-20260416",
            "built_at": "2026-04-16T18:00:00Z",
            "corpora": ["cve", "cwe", "kev"],
            "taxonomy": "cwe-taxonomy.json",
            "sources": {"cve": {"type": "nvd", "feed_date": "2026-04-15"}}
        }"#;
        let m = parse_manifest(json).unwrap();
        assert_eq!(m.bundle_id, "fastrag-20260416");
        assert_eq!(m.corpora, vec!["cve", "cwe", "kev"]);
        assert_eq!(m.taxonomy, "cwe-taxonomy.json");
        assert_eq!(m.built_at, "2026-04-16T18:00:00Z");
    }

    #[test]
    fn parse_manifest_rejects_wrong_schema() {
        let json = r#"{"schema_version": 99, "bundle_id": "x", "built_at": "t",
                       "corpora": [], "taxonomy": "t.json"}"#;
        let err = parse_manifest(json).unwrap_err();
        assert!(matches!(err, BundleError::SchemaMismatch { found: 99 }));
    }

    #[test]
    fn validate_layout_requires_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let err = validate_layout(tmp.path()).unwrap_err();
        assert!(matches!(err, BundleError::ManifestMissing(_)));
    }

    #[test]
    fn validate_layout_requires_all_corpora() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("bundle.json"),
            r#"{
                "schema_version": 1, "bundle_id": "b", "built_at": "t",
                "corpora": ["cve","cwe","kev"], "taxonomy": "cwe-taxonomy.json"
            }"#,
        )
        .unwrap();
        // Only cve present — cwe and kev missing.
        let cve = root.join("corpora/cve");
        std::fs::create_dir_all(&cve).unwrap();
        std::fs::write(cve.join("manifest.json"), "{}").unwrap();
        std::fs::create_dir_all(root.join("taxonomy")).unwrap();
        std::fs::write(root.join("taxonomy/cwe-taxonomy.json"), "{}").unwrap();

        let err = validate_layout(root).unwrap_err();
        match err {
            BundleError::CorpusMissing(name) => {
                assert!(
                    name.starts_with("cwe") || name.starts_with("kev"),
                    "expected missing cwe/kev, got: {name}"
                );
            }
            other => panic!("expected CorpusMissing, got: {other:?}"),
        }
    }

    #[test]
    fn validate_layout_requires_corpus_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("bundle.json"),
            r#"{
                "schema_version": 1, "bundle_id": "b", "built_at": "t",
                "corpora": ["cve","cwe","kev"], "taxonomy": "cwe-taxonomy.json"
            }"#,
        )
        .unwrap();
        for c in ["cve", "cwe", "kev"] {
            std::fs::create_dir_all(root.join("corpora").join(c)).unwrap();
            // Deliberately skip manifest.json.
        }
        std::fs::create_dir_all(root.join("taxonomy")).unwrap();
        std::fs::write(root.join("taxonomy/cwe-taxonomy.json"), "{}").unwrap();

        let err = validate_layout(root).unwrap_err();
        match err {
            BundleError::CorpusMissing(name) => {
                assert!(name.contains("manifest.json"), "got: {name}");
            }
            other => panic!("expected CorpusMissing(..manifest.json..), got: {other:?}"),
        }
    }

    #[test]
    fn validate_layout_requires_taxonomy() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("bundle.json"),
            r#"{
                "schema_version": 1, "bundle_id": "b", "built_at": "t",
                "corpora": ["cve","cwe","kev"], "taxonomy": "cwe-taxonomy.json"
            }"#,
        )
        .unwrap();
        for c in ["cve", "cwe", "kev"] {
            let dir = root.join("corpora").join(c);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("manifest.json"), "{}").unwrap();
        }
        // Taxonomy file missing.
        let err = validate_layout(root).unwrap_err();
        assert!(matches!(err, BundleError::TaxonomyMissing(_)));
    }

    #[test]
    fn load_bundle_populates_state() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("bundle.json"),
            r#"{
                "schema_version": 1, "bundle_id": "b1", "built_at": "t",
                "corpora": ["cve","cwe","kev"], "taxonomy": "cwe-taxonomy.json"
            }"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("taxonomy")).unwrap();
        std::fs::write(
            root.join("taxonomy/cwe-taxonomy.json"),
            r#"{"schema_version":2,"version":"4.15","view":"1000",
                "closure":{"89":[89]},"parents":{"89":[]}}"#,
        )
        .unwrap();
        for c in ["cve", "cwe", "kev"] {
            let dir = root.join("corpora").join(c);
            test_support::write_empty_corpus(&dir).unwrap();
        }

        let state = BundleState::load(root).unwrap();
        assert_eq!(state.manifest.bundle_id, "b1");
        assert_eq!(state.corpora.len(), 3);
        assert!(state.corpora.contains_key("cve"));
        assert!(state.corpora.contains_key("cwe"));
        assert!(state.corpora.contains_key("kev"));
        assert_eq!(state.corpora["cve"].dir(), root.join("corpora/cve"));
        assert_eq!(state.taxonomy.version(), "4.15");
    }

    #[test]
    fn parse_manifest_rejects_empty_corpora() {
        let json = r#"{"schema_version":1,"bundle_id":"b","built_at":"t",
                       "corpora":[],"taxonomy":"t.json"}"#;
        let err = parse_manifest(json).unwrap_err();
        assert!(matches!(err, BundleError::CorpusMissing(_)));
    }

    #[test]
    fn validate_layout_accepts_manifest_declared_corpora() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("bundle.json"),
            r#"{
                "schema_version": 1, "bundle_id": "vams-lookup-v1", "built_at": "t",
                "corpora": ["cwe","kev"], "taxonomy": "cwe-taxonomy.json"
            }"#,
        )
        .unwrap();
        for c in ["cwe", "kev"] {
            let dir = root.join("corpora").join(c);
            test_support::write_empty_corpus(&dir).unwrap();
        }
        std::fs::create_dir_all(root.join("taxonomy")).unwrap();
        std::fs::write(root.join("taxonomy/cwe-taxonomy.json"), "{}").unwrap();

        let manifest = validate_layout(root).unwrap();
        assert_eq!(manifest.corpora, vec!["cwe", "kev"]);
    }

    #[test]
    fn load_bundle_rejects_bad_taxonomy_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("bundle.json"),
            r#"{
                "schema_version": 1, "bundle_id": "b1", "built_at": "t",
                "corpora": ["cve","cwe","kev"], "taxonomy": "cwe-taxonomy.json"
            }"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("taxonomy")).unwrap();
        // schema_version = 1 — unsupported (taxonomy expects v2).
        std::fs::write(
            root.join("taxonomy/cwe-taxonomy.json"),
            r#"{"schema_version":1,"version":"4.0","view":"1000",
                "closure":{},"parents":{}}"#,
        )
        .unwrap();
        for c in ["cve", "cwe", "kev"] {
            let dir = root.join("corpora").join(c);
            test_support::write_empty_corpus(&dir).unwrap();
        }
        let err = BundleState::load(root).unwrap_err();
        assert!(matches!(err, BundleError::Taxonomy(_)), "got: {err:?}");
    }
}
