mod error;
pub mod schema;

pub use error::TantivyIndexError;

use std::path::Path;

use fastrag_index::IndexEntry;
use fastrag_index::fusion::ScoredId;
use fastrag_index::identifiers::SecurityId;
use schema::FieldSet;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, QueryParser, TermQuery};
use tantivy::schema::IndexRecordOption;
use tantivy::schema::Value;
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, Term};

const TANTIVY_SUBDIR: &str = "tantivy_index";
const WRITER_HEAP_MB: usize = 50_000_000; // 50 MB

/// A Tantivy full-text index for BM25 search and exact security ID lookup.
pub struct TantivyIndex {
    index: Index,
    reader: IndexReader,
    fields: FieldSet,
}

impl TantivyIndex {
    /// Create a new Tantivy index at `corpus_dir/tantivy_index/`.
    pub fn create(corpus_dir: &Path) -> Result<Self, TantivyIndexError> {
        let dir = corpus_dir.join(TANTIVY_SUBDIR);
        std::fs::create_dir_all(&dir)?;

        let (schema, fields) = schema::build_schema();
        let index = Index::create_in_dir(&dir, schema)?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;

        Ok(Self {
            index,
            reader,
            fields,
        })
    }

    /// Open an existing Tantivy index from `corpus_dir/tantivy_index/`.
    pub fn open(corpus_dir: &Path) -> Result<Self, TantivyIndexError> {
        let dir = corpus_dir.join(TANTIVY_SUBDIR);
        let index = Index::open_in_dir(&dir)?;
        let (_, fields) = schema::build_schema();
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;

        Ok(Self {
            index,
            reader,
            fields,
        })
    }

    /// Check whether a Tantivy index exists at `corpus_dir/tantivy_index/`.
    pub fn exists(corpus_dir: &Path) -> bool {
        corpus_dir.join(TANTIVY_SUBDIR).join("meta.json").exists()
    }

    /// Add index entries. Call `commit()` afterward to persist.
    pub fn add_entries(&self, entries: &[IndexEntry]) -> Result<(), TantivyIndexError> {
        let mut writer: IndexWriter = self.index.writer(WRITER_HEAP_MB)?;

        for entry in entries {
            let mut doc = tantivy::TantivyDocument::new();
            doc.add_u64(self.fields.id, entry.id);
            doc.add_text(self.fields.chunk_text, &entry.chunk_text);
            // Persist the raw chunk text for display / exact-match. Fall
            // back to `chunk_text` (which is raw on non-contextualized
            // corpora) so the field is always populated.
            doc.add_text(
                self.fields.display_text,
                entry.display_text.as_deref().unwrap_or(&entry.chunk_text),
            );
            doc.add_text(
                self.fields.source_path,
                entry.source_path.to_string_lossy().as_ref(),
            );
            if let Some(ref section) = entry.section {
                doc.add_text(self.fields.section, section);
            }
            // Index security identifiers from metadata
            if let Some(cve) = entry.metadata.get("cve_id") {
                doc.add_text(self.fields.cve_id, cve);
            }
            if let Some(cwe) = entry.metadata.get("cwe") {
                doc.add_text(self.fields.cwe, cwe);
            }
            // Store all metadata as JSON
            let meta_json = serde_json::to_string(&entry.metadata).unwrap_or_default();
            doc.add_text(self.fields.metadata_json, &meta_json);

            writer.add_document(doc)?;
        }

        writer.commit()?;
        self.reader.reload()?;
        Ok(())
    }

    /// Delete entries by chunk ID. Call `commit()` afterward to persist.
    pub fn delete_by_ids(&self, ids: &[u64]) -> Result<(), TantivyIndexError> {
        let mut writer: IndexWriter = self.index.writer(WRITER_HEAP_MB)?;

        for &id in ids {
            writer.delete_term(Term::from_field_u64(self.fields.id, id));
        }

        writer.commit()?;
        self.reader.reload()?;
        Ok(())
    }

    /// BM25 full-text search on `chunk_text`. Returns top-k results as `ScoredId`.
    pub fn bm25_search(
        &self,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<ScoredId>, TantivyIndexError> {
        let searcher = self.reader.searcher();
        let query_parser = QueryParser::for_index(&self.index, vec![self.fields.chunk_text]);
        let parsed = query_parser.parse_query(query)?;

        let top_docs = searcher.search(&parsed, &TopDocs::with_limit(top_k))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let doc: tantivy::TantivyDocument = searcher.doc(doc_address)?;
            if let Some(id_val) = doc.get_first(self.fields.id)
                && let Some(id) = id_val.as_u64()
            {
                results.push(ScoredId { id, score });
            }
        }

        Ok(results)
    }

    /// Exact term lookup for security identifiers (CVE-ID, CWE).
    pub fn exact_lookup(&self, ids: &[SecurityId]) -> Result<Vec<ScoredId>, TantivyIndexError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let searcher = self.reader.searcher();
        let mut term_queries: Vec<(Occur, Box<dyn tantivy::query::Query>)> = Vec::new();

        for id in ids {
            let (field, value) = match id {
                SecurityId::Cve(v) => (self.fields.cve_id, v.as_str()),
                SecurityId::Cwe(v) => (self.fields.cwe, v.as_str()),
            };
            let term = Term::from_field_text(field, value);
            term_queries.push((
                Occur::Should,
                Box::new(TermQuery::new(term, IndexRecordOption::Basic)),
            ));
        }

        let query = BooleanQuery::new(term_queries);
        let top_docs = searcher.search(&query, &TopDocs::with_limit(100))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let doc: tantivy::TantivyDocument = searcher.doc(doc_address)?;
            if let Some(id_val) = doc.get_first(self.fields.id)
                && let Some(id) = id_val.as_u64()
            {
                results.push(ScoredId { id, score });
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn test_entry(id: u64, text: &str, metadata: BTreeMap<String, String>) -> IndexEntry {
        IndexEntry {
            id,
            vector: vec![0.0; 16],
            chunk_text: text.to_string(),
            source_path: PathBuf::from(format!("doc_{id}.txt")),
            chunk_index: 0,
            section: None,
            element_kinds: vec![],
            pages: vec![],
            language: None,
            metadata,
            display_text: None,
        }
    }

    fn empty_meta() -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    fn cve_meta(cve: &str) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("cve_id".to_string(), cve.to_string());
        m
    }

    #[test]
    fn create_and_add_entries() {
        let dir = tempdir().unwrap();
        let idx = TantivyIndex::create(dir.path()).unwrap();
        let entries = vec![
            test_entry(1, "Rust is a systems programming language", empty_meta()),
            test_entry(2, "Python is popular for data science", empty_meta()),
        ];
        idx.add_entries(&entries).unwrap();

        let results = idx.bm25_search("systems programming", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].id, 1);
    }

    #[test]
    fn delete_by_id_removes_from_search() {
        let dir = tempdir().unwrap();
        let idx = TantivyIndex::create(dir.path()).unwrap();
        let entries = vec![
            test_entry(1, "Rust is a systems programming language", empty_meta()),
            test_entry(2, "Python is popular for data science", empty_meta()),
        ];
        idx.add_entries(&entries).unwrap();

        idx.delete_by_ids(&[1]).unwrap();

        let results = idx.bm25_search("systems programming", 10).unwrap();
        assert!(
            results.is_empty() || results.iter().all(|r| r.id != 1),
            "deleted entry should not appear in results"
        );
    }

    #[test]
    fn reopen_after_save() {
        let dir = tempdir().unwrap();
        {
            let idx = TantivyIndex::create(dir.path()).unwrap();
            let entries = vec![test_entry(
                1,
                "Rust is a systems programming language",
                empty_meta(),
            )];
            idx.add_entries(&entries).unwrap();
        }

        assert!(TantivyIndex::exists(dir.path()));

        let idx = TantivyIndex::open(dir.path()).unwrap();
        let results = idx.bm25_search("Rust programming", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].id, 1);
    }

    #[test]
    fn exact_lookup_finds_cve() {
        let dir = tempdir().unwrap();
        let idx = TantivyIndex::create(dir.path()).unwrap();
        let entries = vec![
            test_entry(
                1,
                "A buffer overflow vulnerability",
                cve_meta("CVE-2024-1234"),
            ),
            test_entry(2, "Normal document with no CVE", empty_meta()),
        ];
        idx.add_entries(&entries).unwrap();

        let results = idx
            .exact_lookup(&[SecurityId::Cve("CVE-2024-1234".into())])
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, 1);
    }

    #[test]
    fn exact_lookup_returns_empty_on_miss() {
        let dir = tempdir().unwrap();
        let idx = TantivyIndex::create(dir.path()).unwrap();
        let entries = vec![test_entry(1, "No security IDs here", empty_meta())];
        idx.add_entries(&entries).unwrap();

        let results = idx
            .exact_lookup(&[SecurityId::Cve("CVE-9999-0000".into())])
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn empty_index_returns_empty_results() {
        let dir = tempdir().unwrap();
        let idx = TantivyIndex::create(dir.path()).unwrap();
        // Need to commit even an empty writer for reader to work
        let mut writer: IndexWriter = idx.index.writer(WRITER_HEAP_MB).unwrap();
        writer.commit().unwrap();
        idx.reader.reload().unwrap();

        let results = idx.bm25_search("anything", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn exists_returns_false_for_missing() {
        let dir = tempdir().unwrap();
        assert!(!TantivyIndex::exists(dir.path()));
    }

    #[test]
    fn bm25_ranks_by_relevance() {
        let dir = tempdir().unwrap();
        let idx = TantivyIndex::create(dir.path()).unwrap();
        let entries = vec![
            test_entry(
                1,
                "The quick brown fox jumps over the lazy dog",
                empty_meta(),
            ),
            test_entry(
                2,
                "Rust Rust Rust systems programming language Rust",
                empty_meta(),
            ),
            test_entry(3, "Python is popular for data science", empty_meta()),
        ];
        idx.add_entries(&entries).unwrap();

        let results = idx.bm25_search("Rust programming", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].id, 2, "Rust-heavy doc should rank first");
    }
}
