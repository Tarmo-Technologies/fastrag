use crate::ElementKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexEntry {
    pub id: u64,
    pub vector: Vec<f32>,
    /// Text that was indexed for both BM25 and the dense vector. When the
    /// contextualization stage runs, this is the `"{context}\n\n{raw}"`
    /// form. When contextualization is disabled, this is the raw chunk
    /// text (and `display_text` is `None`).
    pub chunk_text: String,
    pub source_path: PathBuf,
    pub chunk_index: usize,
    pub section: Option<String>,
    pub element_kinds: Vec<ElementKind>,
    pub pages: Vec<usize>,
    pub language: Option<String>,
    /// User-supplied metadata (customer, severity, year, project, ...).
    /// Populated via `.meta.json` sidecar files or `--metadata k=v` at index time.
    /// Empty on older indexes — `#[serde(default)]` keeps them loadable.
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    /// Raw chunk text preserved verbatim for display and CVE/CWE exact-match
    /// lookup. `Some(raw)` when contextualization has prefixed `chunk_text`,
    /// `None` on corpora without contextualization (display falls back to
    /// `chunk_text`, which is raw in that case). `#[serde(default)]` keeps
    /// pre-v2 JSON entries loadable; bincode-serialized entries always
    /// carry the field (its presence tag is a single byte for `None`).
    #[serde(default)]
    pub display_text: Option<String>,
}

impl IndexEntry {
    /// Returns the text appropriate for display to humans / downstream LLMs —
    /// `display_text` if populated (contextualized corpora), otherwise
    /// `chunk_text` (non-contextualized corpora store raw there).
    pub fn display(&self) -> &str {
        self.display_text.as_deref().unwrap_or(&self.chunk_text)
    }
}

impl IndexEntry {
    /// Check whether every `filter` key/value pair is present in the entry's metadata.
    /// An empty filter always matches.
    pub fn matches_filter(&self, filter: &BTreeMap<String, String>) -> bool {
        filter
            .iter()
            .all(|(k, v)| self.metadata.get(k).map(|m| m == v).unwrap_or(false))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchHit {
    pub entry: IndexEntry,
    pub score: f32, // cosine similarity
}
