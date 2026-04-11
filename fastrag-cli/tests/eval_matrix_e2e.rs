#![cfg(all(feature = "eval", feature = "contextual", feature = "rerank"))]

use std::path::PathBuf;

use assert_cmd::Command;
use tempfile::tempdir;

/// Full matrix e2e: indexes the mini fixture twice (ctx + raw), runs
/// `fastrag eval --config-matrix`, and validates the JSON report shape.
///
/// Skipped unless FASTRAG_LLAMA_TEST=1 AND FASTRAG_RERANK_TEST=1.
/// These env vars gate tests that require live model servers.
#[test]
#[ignore = "requires llama-server + ONNX reranker model files"]
fn eval_matrix_runs_four_variants_and_writes_report() {
    if std::env::var("FASTRAG_LLAMA_TEST").as_deref() != Ok("1") {
        eprintln!("FASTRAG_LLAMA_TEST not set — skipping");
        return;
    }
    if std::env::var("FASTRAG_RERANK_TEST").as_deref() != Ok("1") {
        eprintln!("FASTRAG_RERANK_TEST not set — skipping");
        return;
    }

    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/eval_mini");
    let corpus_src = fixture_dir.join("corpus");
    let questions = fixture_dir.join("questions.json");

    let tmp = tempdir().expect("tempdir");
    let ctx_corpus = tmp.path().join("ctx_corpus");
    let raw_corpus = tmp.path().join("raw_corpus");
    let report_path = tmp.path().join("matrix_report.json");

    // Index with contextualize.
    Command::cargo_bin("fastrag")
        .unwrap()
        .args([
            "index",
            corpus_src.to_str().unwrap(),
            "--corpus",
            ctx_corpus.to_str().unwrap(),
            "--contextualize",
        ])
        .assert()
        .success();

    // Index without contextualize.
    Command::cargo_bin("fastrag")
        .unwrap()
        .args([
            "index",
            corpus_src.to_str().unwrap(),
            "--corpus",
            raw_corpus.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Run the matrix eval.
    Command::cargo_bin("fastrag")
        .unwrap()
        .args([
            "eval",
            "--config-matrix",
            "--gold-set",
            questions.to_str().unwrap(),
            "--corpus",
            ctx_corpus.to_str().unwrap(),
            "--corpus-no-contextual",
            raw_corpus.to_str().unwrap(),
            "--report",
            report_path.to_str().unwrap(),
            "--top-k",
            "5",
        ])
        .assert()
        .success();

    // Parse and validate the report shape.
    let raw = std::fs::read(&report_path).expect("report file must exist");
    let report: serde_json::Value =
        serde_json::from_slice(&raw).expect("report must be valid JSON");

    let runs = report["runs"].as_array().expect("runs must be array");
    assert_eq!(runs.len(), 4, "must have exactly 4 variant runs");

    for run in runs {
        let hit5 = run["hit_at_5"].as_f64().expect("hit_at_5 must be f64");
        assert!(
            hit5.is_finite() && (0.0..=1.0).contains(&hit5),
            "hit_at_5 must be in [0,1], got {hit5}"
        );

        let per_q = run["per_question"]
            .as_array()
            .expect("per_question must be array");
        assert_eq!(per_q.len(), 10, "each variant must score all 10 questions");

        let p50 = run["latency"]["total"]["p50_us"]
            .as_u64()
            .expect("p50_us must be u64");
        assert!(p50 > 0, "p50_us must be > 0, got {p50}");
    }

    let rerank_delta = report["rerank_delta"]
        .as_f64()
        .expect("rerank_delta must be f64");
    assert!(rerank_delta.is_finite(), "rerank_delta must be finite");

    let contextual_delta = report["contextual_delta"]
        .as_f64()
        .expect("contextual_delta must be f64");
    assert!(
        contextual_delta.is_finite(),
        "contextual_delta must be finite"
    );

    let hybrid_delta = report["hybrid_delta"]
        .as_f64()
        .expect("hybrid_delta must be f64");
    assert!(hybrid_delta.is_finite(), "hybrid_delta must be finite");
}
