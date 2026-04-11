//! Integration test: a flaky contextualizer in non-strict mode must leave
//! every chunk in the slice and split them into (ctx-prefixed) vs (raw)
//! without losing any chunk.
#![cfg(feature = "test-utils")]

use fastrag_context::test_utils::MockContextualizer;
use fastrag_context::{ContextCache, run_contextualize_stage};
use fastrag_core::Chunk;

fn chunk(text: &str, i: usize) -> Chunk {
    Chunk {
        elements: vec![],
        text: text.to_string(),
        char_count: text.chars().count(),
        section: None,
        index: i,
        contextualized_text: None,
    }
}

#[test]
fn stage_fallback_non_strict_preserves_all_chunks() {
    let dir = tempfile::tempdir().unwrap();
    let mut cache = ContextCache::open(&dir.path().join("c.sqlite")).unwrap();
    let ctx = MockContextualizer::fail_every(3);

    let mut chunks: Vec<Chunk> = (0..15).map(|i| chunk(&format!("chunk-{i}"), i)).collect();

    let stats = run_contextualize_stage(&ctx, &mut cache, "DocTitle", &mut chunks, false)
        .expect("non-strict should not return Err");

    // All 15 chunks remain in the slice — none dropped.
    assert_eq!(chunks.len(), 15);

    // 2/3 succeed, 1/3 fall back.
    assert_eq!(stats.ok, 10);
    assert_eq!(stats.failed, 5);
    assert_eq!(stats.total(), 15);

    let with_ctx = chunks
        .iter()
        .filter(|c| c.contextualized_text.is_some())
        .count();
    assert_eq!(with_ctx, 10);

    // All chunks still have their raw `text` intact — none mutated.
    for (i, c) in chunks.iter().enumerate() {
        assert_eq!(c.text, format!("chunk-{i}"));
    }

    // Cache persists the split.
    let failed_rows: Vec<_> = cache.iter_failed().unwrap().collect();
    assert_eq!(failed_rows.len(), 5);

    let (ok_count, failed_count) = cache.row_count().unwrap();
    assert_eq!(ok_count, 10);
    assert_eq!(failed_count, 5);

    // Every failed row carries doc_title so --retry-failed can run alone.
    for row in failed_rows {
        assert_eq!(row.doc_title, "DocTitle");
        assert!(!row.raw_text.is_empty());
    }
}
