//! End-to-end: post-rerank multiplicative decay promotes fresh docs over
//! equally-relevant stale docs.
#![cfg(feature = "store")]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use fastrag::ChunkingStrategy;
use fastrag::corpus::hybrid::HybridOpts;
use fastrag::corpus::temporal::{Strength, TemporalPolicy};
use fastrag::corpus::{LatencyBreakdown, QueryOpts, query_corpus_reranked_opts};
use fastrag::ingest::engine::index_jsonl;
use fastrag::ingest::jsonl::JsonlIngestConfig;
use fastrag_embed::DynEmbedderTrait;
use fastrag_embed::test_utils::MockEmbedder;
use fastrag_rerank::test_utils::MockReranker;
use fastrag_store::schema::TypedKind;

fn write_fixture(path: &Path) {
    // All three docs contain the query tokens "openssl heap overflow" so
    // BM25 and dense both rank them closely. The trailing discriminator
    // word (legacy/recent/dateless) lets us identify each hit by chunk_text.
    let lines = [
        r#"{"id":"A","text":"openssl heap overflow legacy","published_date":"2016-01-01"}"#,
        r#"{"id":"B","text":"openssl heap overflow recent","published_date":"2026-04-01"}"#,
        r#"{"id":"C","text":"openssl heap overflow dateless"}"#,
    ];
    fs::write(path, lines.join("\n")).unwrap();
}

fn cfg() -> JsonlIngestConfig {
    JsonlIngestConfig {
        text_fields: vec!["text".into()],
        id_field: "id".into(),
        metadata_fields: vec!["published_date".into()],
        metadata_types: BTreeMap::from([("published_date".into(), TypedKind::Date)]),
        array_fields: vec![],
        cwe_field: None,
    }
}

fn chunking() -> ChunkingStrategy {
    ChunkingStrategy::Basic {
        max_characters: 500,
        overlap: 0,
    }
}

#[cfg(feature = "rerank")]
#[test]
fn decay_promotes_fresh_over_equally_relevant_stale() {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    let jsonl = tmp.path().join("docs.jsonl");
    write_fixture(&jsonl);

    let embedder = MockEmbedder;
    index_jsonl(
        &jsonl,
        &corpus,
        &chunking(),
        &embedder as &dyn DynEmbedderTrait,
        &cfg(),
    )
    .unwrap();

    // Late-stage temporal decay via query_corpus_reranked_opts.
    // FavorRecent(Medium) → 180d halflife, 0.60 floor.
    // Age(B) ≈ 15d → factor near 1.0.
    // Age(A) = ~3756d → factor saturates to floor 0.60.
    // C (dateless) → prior 1.0 (score unchanged).
    let opts = QueryOpts {
        hybrid: HybridOpts::default(),
        temporal_policy: TemporalPolicy::FavorRecent(Strength::Medium),
        date_fields: vec!["published_date".into()],
        ..Default::default()
    };

    let reranker = MockReranker;
    let mut bd = LatencyBreakdown::default();
    let hits = query_corpus_reranked_opts(
        &corpus,
        "openssl heap overflow",
        3,
        4,
        &embedder as &dyn DynEmbedderTrait,
        &reranker as &dyn fastrag_rerank::Reranker,
        None,
        &opts,
        &mut bd,
        0,
    )
    .unwrap();

    assert_eq!(hits.len(), 3, "expected 3 hits, got {}", hits.len());

    // Find scores by doc discriminator.
    let fresh_score = hits
        .iter()
        .find(|h| h.chunk_text.contains("recent"))
        .map(|h| h.score)
        .expect("recent doc missing");
    let old_score = hits
        .iter()
        .find(|h| h.chunk_text.contains("legacy"))
        .map(|h| h.score)
        .expect("legacy doc missing");

    // Strong assertion: fresh doc outranks stale after decay.
    assert!(
        fresh_score > old_score,
        "fresh doc must outrank stale doc; fresh={fresh_score} old={old_score}"
    );

    // Old doc score must be at or below floor * reranker_score.
    // MockReranker assigns 3.0 to all (same 3-token overlap), floor=0.60.
    let expected_max_old = 3.0 * 0.60 + 1e-3;
    assert!(
        old_score <= expected_max_old,
        "old doc must be at or below floor * reranker_score ({expected_max_old}); got {old_score}"
    );
}
