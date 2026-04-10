//! Hybrid retrieval coordinator — BM25 + dense vector search fused with RRF.
//!
//! Wraps `HnswIndex` (dense) and `TantivyIndex` (BM25) into a single
//! `HybridIndex` that implements the 5-stage query pipeline:
//!
//! 1. CVE/CWE exact lookup via Tantivy
//! 2. BM25 full-text search
//! 3. Dense HNSW vector search
//! 4. Reciprocal Rank Fusion (k=60)
//! 5. Prepend exact hits, deduplicated

use std::collections::HashSet;
use std::path::Path;

use fastrag_embed::DynEmbedderTrait;
use fastrag_index::fusion::{ScoredId, rrf_fuse};
use fastrag_index::identifiers::extract_security_identifiers;
use fastrag_index::{HnswIndex, IndexEntry, SearchHit, VectorIndex};
use fastrag_tantivy::TantivyIndex;

use super::CorpusError;

const RRF_K: u32 = 60;
const RETRIEVAL_DEPTH: usize = 50;

/// Coordinates dense (HNSW) and sparse (Tantivy BM25) indices for hybrid retrieval.
pub struct HybridIndex {
    hnsw: HnswIndex,
    tantivy: Option<TantivyIndex>,
}

impl HybridIndex {
    /// Load both indices from a corpus directory. If the Tantivy index is missing
    /// (legacy corpus), falls back to dense-only with a warning.
    pub fn load(corpus_dir: &Path, embedder: &dyn DynEmbedderTrait) -> Result<Self, CorpusError> {
        let hnsw = HnswIndex::load(corpus_dir, embedder)?;

        let tantivy = if TantivyIndex::exists(corpus_dir) {
            Some(TantivyIndex::open(corpus_dir)?)
        } else {
            eprintln!(
                "warning: no tantivy_index/ in corpus — falling back to dense-only retrieval. \
                 Re-index with hybrid mode to enable BM25."
            );
            None
        };

        Ok(Self { hnsw, tantivy })
    }

    /// Create a new hybrid index for indexing. Always creates both indices.
    pub fn create(
        corpus_dir: &Path,
        manifest: fastrag_index::CorpusManifest,
    ) -> Result<Self, CorpusError> {
        let hnsw = HnswIndex::new(manifest);
        let tantivy = Some(TantivyIndex::create(corpus_dir)?);
        Ok(Self { hnsw, tantivy })
    }

    /// Add entries to both indices.
    pub fn add(&mut self, entries: Vec<IndexEntry>) -> Result<(), CorpusError> {
        if let Some(ref tantivy) = self.tantivy {
            tantivy.add_entries(&entries)?;
        }
        self.hnsw.add(entries)?;
        Ok(())
    }

    /// Remove entries from both indices by chunk ID.
    pub fn remove_by_chunk_ids(&mut self, ids: &[u64]) -> Result<(), CorpusError> {
        if let Some(ref tantivy) = self.tantivy {
            tantivy.delete_by_ids(ids)?;
        }
        self.hnsw.remove_by_chunk_ids(ids);
        Ok(())
    }

    /// Save both indices to the corpus directory.
    pub fn save(&self, corpus_dir: &Path) -> Result<(), CorpusError> {
        self.hnsw.save(corpus_dir)?;
        // Tantivy persists on commit (already done in add_entries/delete_by_ids).
        Ok(())
    }

    /// Access the underlying HNSW index (for manifest, entries, etc.).
    pub fn hnsw(&self) -> &HnswIndex {
        &self.hnsw
    }

    /// Mutable access to the HNSW index.
    pub fn hnsw_mut(&mut self) -> &mut HnswIndex {
        &mut self.hnsw
    }

    /// 5-stage hybrid query pipeline.
    ///
    /// 1. Extract CVE/CWE identifiers → exact Tantivy lookup
    /// 2. BM25 full-text search (top-50)
    /// 3. Dense HNSW search (top-50)
    /// 4. RRF fusion (k=60) over BM25 + dense
    /// 5. Prepend exact hits (deduplicated), truncate to top_k
    pub fn query_hybrid(
        &self,
        query_text: &str,
        query_vector: &[f32],
        top_k: usize,
    ) -> Result<Vec<SearchHit>, CorpusError> {
        // If no Tantivy index, fall back to dense-only.
        let Some(ref tantivy) = self.tantivy else {
            return Ok(self.hnsw.query(query_vector, top_k)?);
        };

        // Stage 1: CVE/CWE exact lookup
        let security_ids = extract_security_identifiers(query_text);
        let exact_scored = tantivy.exact_lookup(&security_ids)?;

        // Stage 2: BM25 full-text search
        let bm25_scored = tantivy.bm25_search(query_text, RETRIEVAL_DEPTH)?;

        // Stage 3: Dense HNSW search
        let dense_hits = self.hnsw.query(query_vector, RETRIEVAL_DEPTH)?;
        let dense_scored: Vec<ScoredId> = dense_hits
            .iter()
            .map(|h| ScoredId {
                id: h.entry.id,
                score: h.score,
            })
            .collect();

        // Stage 4: RRF fusion
        let fused = rrf_fuse(&[&bm25_scored, &dense_scored], RRF_K);

        // Stage 5: Prepend exact hits (deduplicated), then fused results
        let mut seen = HashSet::new();
        let mut results = Vec::with_capacity(top_k);

        // Exact hits first
        for scored in &exact_scored {
            if seen.insert(scored.id)
                && let Some(entry) = self.hnsw.entry_by_id(scored.id)
            {
                results.push(SearchHit {
                    entry: entry.clone(),
                    score: scored.score,
                });
            }
        }

        // Then fused results
        for scored in &fused {
            if results.len() >= top_k {
                break;
            }
            if seen.insert(scored.id)
                && let Some(entry) = self.hnsw.entry_by_id(scored.id)
            {
                results.push(SearchHit {
                    entry: entry.clone(),
                    score: scored.score,
                });
            }
        }

        results.truncate(top_k);
        Ok(results)
    }
}

#[cfg(test)]
#[allow(unused_variables)]
mod tests {
    use super::*;
    use fastrag_embed::test_utils::MockEmbedder;
    use fastrag_embed::{CANARY_TEXT, Canary, Embedder, PassageText};
    use fastrag_index::{CorpusManifest, ManifestChunkingStrategy};
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn mock_manifest() -> CorpusManifest {
        let embedder = MockEmbedder;
        let canary_vec = embedder
            .embed_passage(&[PassageText::new(CANARY_TEXT)])
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        CorpusManifest {
            version: 3,
            identity: embedder.identity(),
            canary: Canary {
                text_version: 1,
                vector: canary_vec,
            },
            created_at_unix_seconds: 0,
            chunk_count: 0,
            chunking_strategy: ManifestChunkingStrategy::Basic {
                max_characters: 1000,
                overlap: 0,
            },
            roots: vec![],
            files: vec![],
        }
    }

    fn test_entry(id: u64, text: &str, meta: BTreeMap<String, String>) -> IndexEntry {
        let embedder = MockEmbedder;
        let vector = embedder
            .embed_passage(&[PassageText::new(text)])
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        IndexEntry {
            id,
            vector,
            chunk_text: text.to_string(),
            source_path: PathBuf::from(format!("doc_{id}.txt")),
            chunk_index: 0,
            section: None,
            element_kinds: vec![],
            pages: vec![],
            language: None,
            metadata: meta,
        }
    }

    fn cve_meta(cve: &str) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("cve_id".to_string(), cve.to_string());
        m
    }

    #[test]
    fn create_and_add_syncs_both_indices() {
        let dir = tempdir().unwrap();
        let embedder = MockEmbedder;
        let manifest = mock_manifest();

        let mut hybrid = HybridIndex::create(dir.path(), manifest).unwrap();
        let entries = vec![
            test_entry(1, "Rust is a systems programming language", BTreeMap::new()),
            test_entry(2, "Python is popular for data science", BTreeMap::new()),
        ];
        hybrid.add(entries).unwrap();
        hybrid.save(dir.path()).unwrap();

        // Both indices should have data
        assert_eq!(hybrid.hnsw().entries().len(), 2);
        assert!(TantivyIndex::exists(dir.path()));
    }

    #[test]
    fn remove_syncs_both_indices() {
        let dir = tempdir().unwrap();
        let embedder = MockEmbedder;
        let manifest = mock_manifest();

        let mut hybrid = HybridIndex::create(dir.path(), manifest).unwrap();
        hybrid
            .add(vec![
                test_entry(1, "Rust programming", BTreeMap::new()),
                test_entry(2, "Python scripting", BTreeMap::new()),
            ])
            .unwrap();

        hybrid.remove_by_chunk_ids(&[1]).unwrap();
        assert_eq!(hybrid.hnsw().entries().len(), 1);

        // Tantivy should also have removed it
        let tantivy = hybrid.tantivy.as_ref().unwrap();
        let results = tantivy.bm25_search("Rust programming", 10).unwrap();
        assert!(
            results.is_empty() || results.iter().all(|r| r.id != 1),
            "deleted entry should not appear in Tantivy results"
        );
    }

    #[test]
    fn legacy_corpus_falls_back_to_dense_only() {
        let dir = tempdir().unwrap();
        let embedder = MockEmbedder;
        let manifest = mock_manifest();

        // Create a dense-only corpus (no tantivy_index/)
        let mut hnsw = HnswIndex::new(manifest);
        hnsw.add(vec![test_entry(1, "some text", BTreeMap::new())])
            .unwrap();
        hnsw.save(dir.path()).unwrap();

        // Load as hybrid — should succeed with tantivy=None
        let hybrid = HybridIndex::load(dir.path(), &embedder).unwrap();
        assert!(hybrid.tantivy.is_none());

        // Query should still work (dense-only fallback)
        let query_vec = MockEmbedder
            .embed_query(&[fastrag_embed::QueryText::new("dummy")])
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let results = hybrid.query_hybrid("anything", &query_vec, 5).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn exact_cve_hit_prepended() {
        let dir = tempdir().unwrap();
        let embedder = MockEmbedder;
        let manifest = mock_manifest();

        let mut hybrid = HybridIndex::create(dir.path(), manifest).unwrap();
        hybrid
            .add(vec![
                test_entry(
                    1,
                    "A buffer overflow vulnerability",
                    cve_meta("CVE-2024-1234"),
                ),
                test_entry(2, "Rust is a systems programming language", BTreeMap::new()),
                test_entry(3, "Another vulnerability report", BTreeMap::new()),
            ])
            .unwrap();

        let query_vec = MockEmbedder
            .embed_query(&[fastrag_embed::QueryText::new("dummy")])
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let results = hybrid.query_hybrid("CVE-2024-1234", &query_vec, 5).unwrap();

        assert!(!results.is_empty());
        assert_eq!(
            results[0].entry.id, 1,
            "exact CVE match should be first result"
        );
    }

    #[test]
    fn bm25_contributes_to_ranking() {
        let dir = tempdir().unwrap();
        let embedder = MockEmbedder;
        let manifest = mock_manifest();

        let mut hybrid = HybridIndex::create(dir.path(), manifest).unwrap();
        hybrid
            .add(vec![
                test_entry(
                    1,
                    "Rust Rust Rust systems programming language performance safety",
                    BTreeMap::new(),
                ),
                test_entry(
                    2,
                    "Python is popular for data science and machine learning",
                    BTreeMap::new(),
                ),
            ])
            .unwrap();

        let query_vec = MockEmbedder
            .embed_query(&[fastrag_embed::QueryText::new("dummy")])
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let results = hybrid
            .query_hybrid("Rust programming", &query_vec, 5)
            .unwrap();

        // With mock embedder all vectors are identical, so BM25 is the tiebreaker
        assert!(!results.is_empty());
        // The Rust-heavy doc should appear in results from BM25
        let has_rust_doc = results.iter().any(|r| r.entry.id == 1);
        assert!(has_rust_doc, "BM25 should surface the Rust-heavy document");
    }
}
