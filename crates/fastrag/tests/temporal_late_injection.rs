//! Integration test: late-stage temporal decay injection post-rerank.
//!
//! Verifies that `query_corpus_reranked_opts` applies `TemporalPolicy` AFTER
//! the reranker runs, so the contract is: decay touches scores that the
//! reranker already saw, and old docs are penalised even if lexically strong.
#![cfg(all(feature = "store", feature = "rerank"))]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use fastrag::ChunkingStrategy;
use fastrag::corpus::temporal::{Strength, TemporalPolicy};
use fastrag::corpus::{LatencyBreakdown, QueryOpts, query_corpus_reranked_opts};
use fastrag::ingest::engine::index_jsonl;
use fastrag::ingest::jsonl::JsonlIngestConfig;
use fastrag_embed::DynEmbedderTrait;
use fastrag_embed::test_utils::MockEmbedder;
use fastrag_rerank::test_utils::MockReranker;
use fastrag_store::schema::TypedKind;

fn write_fixture(path: &Path) {
    // All three docs share the query tokens so MockReranker gives them equal
    // lexical-overlap scores. Dense embedding (MockEmbedder → deterministic)
    // may break ties arbitrarily, but temporal decay should dominate.
    let lines = [
        r#"{"id":"A","text":"openssl heap overflow legacy","published_date":"2021-01-01"}"#,
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

/// With `FavorRecent(Medium)` the old doc (A, 2021-01-01) must have a lower
/// score than the fresh doc (B, 2026-04-01) after late decay, regardless of
/// what the reranker assigned.  Dateless doc (C) gets the 1.0 prior so its
/// score is unchanged from the reranker output.
#[test]
fn favor_recent_medium_decays_old_doc_after_rerank() {
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

    // Baseline: Off policy — collect scores before decay.
    let opts_off = QueryOpts {
        temporal_policy: TemporalPolicy::Off,
        date_fields: vec!["published_date".into()],
        ..Default::default()
    };
    let reranker = MockReranker;
    let mut bd = LatencyBreakdown::default();
    let hits_off = query_corpus_reranked_opts(
        &corpus,
        "openssl heap overflow",
        3,
        4,
        &embedder as &dyn DynEmbedderTrait,
        &reranker as &dyn fastrag_rerank::Reranker,
        None,
        &opts_off,
        &mut bd,
        0,
    )
    .unwrap();

    assert_eq!(hits_off.len(), 3, "expected 3 hits from Off run");

    // Find old doc score in Off run.
    let old_score_off = hits_off
        .iter()
        .find(|h| h.chunk_text.contains("legacy"))
        .map(|h| h.score)
        .expect("legacy doc must appear in Off run");

    // Decay run: FavorRecent(Medium).
    let opts_decay = QueryOpts {
        temporal_policy: TemporalPolicy::FavorRecent(Strength::Medium),
        date_fields: vec!["published_date".into()],
        ..Default::default()
    };
    let mut bd2 = LatencyBreakdown::default();
    let hits_decay = query_corpus_reranked_opts(
        &corpus,
        "openssl heap overflow",
        3,
        4,
        &embedder as &dyn DynEmbedderTrait,
        &reranker as &dyn fastrag_rerank::Reranker,
        None,
        &opts_decay,
        &mut bd2,
        0,
    )
    .unwrap();

    assert_eq!(hits_decay.len(), 3, "expected 3 hits from decay run");

    let old_score_decay = hits_decay
        .iter()
        .find(|h| h.chunk_text.contains("legacy"))
        .map(|h| h.score)
        .expect("legacy doc must appear in decay run");

    let fresh_score_decay = hits_decay
        .iter()
        .find(|h| h.chunk_text.contains("recent"))
        .map(|h| h.score)
        .expect("recent doc must appear in decay run");

    // Core contract: old doc's score is reduced by decay.
    assert!(
        old_score_decay < old_score_off,
        "old doc score must decrease under FavorRecent(Medium): off={old_score_off} decay={old_score_decay}"
    );

    // Fresh doc must outrank old doc after decay.
    assert!(
        fresh_score_decay > old_score_decay,
        "fresh doc must outrank old doc after decay: fresh={fresh_score_decay} old={old_score_decay}"
    );
}
