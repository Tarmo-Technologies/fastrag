//! /similar + verify integration: ANN-surviving candidates filtered by Jaccard.
#![cfg(feature = "store")]

use std::collections::BTreeMap;

use fastrag::ChunkingStrategy;
use fastrag::corpus::verify::{VerifyConfig, VerifyMethod};
use fastrag::corpus::{SimilarityRequest, similarity_search};
use fastrag::ingest::engine::index_jsonl;
use fastrag::ingest::jsonl::JsonlIngestConfig;
use fastrag_embed::test_utils::MockEmbedder;

fn build_corpus(docs: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let jsonl = tmp.path().join("docs.jsonl");
    let lines: Vec<String> = docs
        .iter()
        .map(|(id, body)| serde_json::json!({ "id": id, "body": body }).to_string())
        .collect();
    std::fs::write(&jsonl, lines.join("\n")).unwrap();
    let corpus = tmp.path().join("corpus");
    let cfg = JsonlIngestConfig {
        text_fields: vec!["body".into()],
        id_field: "id".into(),
        metadata_fields: vec![],
        metadata_types: BTreeMap::new(),
        array_fields: vec![],
        cwe_field: None,
    };
    index_jsonl(
        &jsonl,
        &corpus,
        &ChunkingStrategy::Basic {
            max_characters: 500,
            overlap: 0,
        },
        &MockEmbedder as &dyn fastrag_embed::DynEmbedderTrait,
        &cfg,
    )
    .unwrap();
    (tmp, corpus)
}

#[test]
fn verify_none_is_no_op() {
    let (_t, corpus) = build_corpus(&[("a", "alpha"), ("b", "alpha extra unrelated words here")]);
    let req = SimilarityRequest {
        text: "alpha".into(),
        threshold: 0.3,
        max_results: 10,
        targets: vec![("default".into(), corpus)],
        filter: None,
        snippet_len: 0,
        overfetch_cap: 10_000,
        verify: None,
    };
    let resp = similarity_search(&MockEmbedder, &req).unwrap();
    assert_eq!(resp.hits.len(), 2);
    assert!(resp.hits.iter().all(|h| h.verify_score.is_none()));
    assert_eq!(resp.stats.dropped_by_verifier, 0);
}

#[test]
fn verify_threshold_drops_non_dupes() {
    // "a" is a near-dup of the query; "b" shares the "alpha" token with the
    // query but adds ~40 chars of unrelated text, so its char-5gram shingle
    // set has low Jaccard overlap with the query's tiny shingle set.
    let (_t, corpus) = build_corpus(&[
        ("a", "alpha"),
        (
            "b",
            "alpha zzz qqq xxx yyy completely different tail content here",
        ),
    ]);
    let req = SimilarityRequest {
        text: "alpha".into(),
        threshold: 0.3,
        max_results: 10,
        targets: vec![("default".into(), corpus)],
        filter: None,
        snippet_len: 0,
        overfetch_cap: 10_000,
        verify: Some(VerifyConfig {
            method: VerifyMethod::MinHash,
            threshold: 0.7,
        }),
    };
    let resp = similarity_search(&MockEmbedder, &req).unwrap();
    assert_eq!(resp.hits.len(), 1, "only near-dup 'a' should survive");
    assert!(resp.hits[0].verify_score.unwrap() >= 0.7);
    assert_eq!(resp.stats.dropped_by_verifier, 1);
}

#[test]
fn verify_threshold_zero_keeps_all() {
    let (_t, corpus) = build_corpus(&[("a", "alpha"), ("b", "alpha other words")]);
    let req = SimilarityRequest {
        text: "alpha".into(),
        threshold: 0.3,
        max_results: 10,
        targets: vec![("default".into(), corpus)],
        filter: None,
        snippet_len: 0,
        overfetch_cap: 10_000,
        verify: Some(VerifyConfig {
            method: VerifyMethod::MinHash,
            threshold: 0.0,
        }),
    };
    let resp = similarity_search(&MockEmbedder, &req).unwrap();
    assert_eq!(resp.hits.len(), 2);
    assert!(resp.hits.iter().all(|h| h.verify_score.is_some()));
    assert_eq!(resp.stats.dropped_by_verifier, 0);
}

#[test]
fn dropped_aggregates_across_corpora() {
    let (_t1, c1) = build_corpus(&[
        ("x", "alpha"),
        (
            "y",
            "alpha totally unrelated content padding to push the shingle set far from query",
        ),
    ]);
    let (_t2, c2) = build_corpus(&[
        ("x", "alpha"),
        (
            "y",
            "alpha never seen vocabulary ballast filling the chunk out beyond the query length",
        ),
    ]);
    let req = SimilarityRequest {
        text: "alpha".into(),
        threshold: 0.3,
        max_results: 10,
        targets: vec![("one".into(), c1), ("two".into(), c2)],
        filter: None,
        snippet_len: 0,
        overfetch_cap: 10_000,
        verify: Some(VerifyConfig {
            method: VerifyMethod::MinHash,
            threshold: 0.7,
        }),
    };
    let resp = similarity_search(&MockEmbedder, &req).unwrap();
    assert_eq!(resp.stats.dropped_by_verifier, 2);
    for per in resp.stats.per_corpus.values() {
        assert_eq!(per.dropped_by_verifier, 1);
    }
}
