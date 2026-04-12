//! Integration test: ingest an NVD feed file into a corpus via `index_path`,
//! then verify that all 5 CVE chunks are present with expected metadata.
//!
//! Gated behind `#[cfg(feature = "nvd")]` so it does not run in the default
//! (no-nvd) build.

#![cfg(feature = "nvd")]

use fastrag::ChunkingStrategy;
use fastrag::corpus::{LatencyBreakdown, index_path, query_corpus};
use fastrag_embed::test_utils::MockEmbedder;
use tempfile::tempdir;

fn nvd_fixture() -> std::path::PathBuf {
    // crates/fastrag/tests/ → crates/fastrag-nvd/fixtures/
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/fastrag-nvd/fixtures/nvd_slice.json")
}

fn chunking() -> ChunkingStrategy {
    ChunkingStrategy::Basic {
        max_characters: 4096,
        overlap: 0,
    }
}

#[test]
fn nvd_feed_ingest_produces_five_chunks() {
    let input_dir = tempdir().unwrap();
    let corpus_dir = tempdir().unwrap();

    // Copy the fixture into a temp input dir so `walk_for_plan` picks it up.
    let dest = input_dir.path().join("nvd_slice.json");
    std::fs::copy(nvd_fixture(), &dest).expect("fixture copy failed");

    let stats = index_path(
        input_dir.path(),
        corpus_dir.path(),
        &chunking(),
        &MockEmbedder,
    )
    .expect("index_path must succeed on NVD feed");

    assert_eq!(
        stats.chunk_count, 5,
        "expected 5 chunks (one per CVE), got {}",
        stats.chunk_count
    );
}

#[test]
fn nvd_feed_chunks_carry_cve_ids_in_metadata() {
    let input_dir = tempdir().unwrap();
    let corpus_dir = tempdir().unwrap();

    let dest = input_dir.path().join("nvd_slice.json");
    std::fs::copy(nvd_fixture(), &dest).expect("fixture copy failed");

    index_path(
        input_dir.path(),
        corpus_dir.path(),
        &chunking(),
        &MockEmbedder,
    )
    .expect("index_path must succeed");

    // Query with a term likely to match CVE-2021-44228 (Log4Shell).
    let hits = query_corpus(
        corpus_dir.path(),
        "log4j remote code execution",
        5,
        &MockEmbedder,
        &mut LatencyBreakdown::default(),
    )
    .expect("query must succeed");

    assert!(
        !hits.is_empty(),
        "expected at least one hit for log4j query"
    );

    // Every returned entry's metadata must carry a cve_id key.
    for hit in &hits {
        assert!(
            hit.entry.metadata.contains_key("cve_id"),
            "chunk from {:?} missing cve_id metadata",
            hit.entry.source_path
        );
    }
}

#[test]
fn nvd_feed_log4shell_metadata_values() {
    let input_dir = tempdir().unwrap();
    let corpus_dir = tempdir().unwrap();

    let dest = input_dir.path().join("nvd_slice.json");
    std::fs::copy(nvd_fixture(), &dest).expect("fixture copy failed");

    index_path(
        input_dir.path(),
        corpus_dir.path(),
        &chunking(),
        &MockEmbedder,
    )
    .expect("index_path must succeed");

    // Over-fetch all 5 to find log4shell.
    let hits = query_corpus(
        corpus_dir.path(),
        "log4j",
        5,
        &MockEmbedder,
        &mut LatencyBreakdown::default(),
    )
    .expect("query must succeed");

    let log4shell = hits
        .iter()
        .find(|h| h.entry.metadata.get("cve_id").map(String::as_str) == Some("CVE-2021-44228"));

    assert!(
        log4shell.is_some(),
        "CVE-2021-44228 chunk missing from results; hits: {:?}",
        hits.iter()
            .map(|h| h.entry.metadata.get("cve_id").cloned())
            .collect::<Vec<_>>()
    );

    let entry = &log4shell.unwrap().entry;
    assert_eq!(
        entry.metadata.get("cpe_vendor").map(String::as_str),
        Some("apache"),
        "expected cpe_vendor=apache"
    );
    assert_eq!(
        entry.metadata.get("cpe_product").map(String::as_str),
        Some("log4j"),
        "expected cpe_product=log4j"
    );
}

#[test]
fn plain_txt_ingest_still_works_after_nvd_wiring() {
    // Regression: single-doc parsers must not be affected.
    let input_dir = tempdir().unwrap();
    let corpus_dir = tempdir().unwrap();

    std::fs::write(
        input_dir.path().join("readme.txt"),
        "This is a plain text document with some content.\n",
    )
    .unwrap();

    let stats = index_path(
        input_dir.path(),
        corpus_dir.path(),
        &chunking(),
        &MockEmbedder,
    )
    .expect("plain txt ingest must succeed");

    assert!(
        stats.chunk_count >= 1,
        "expected at least 1 chunk from .txt"
    );
}
