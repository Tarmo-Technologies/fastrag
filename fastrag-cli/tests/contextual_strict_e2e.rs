//! End-to-end test for `--context-strict` aborting on the first failure.
//!
//! Injects a single contextualizer failure and asserts that strict mode
//! exits non-zero and never finalizes the corpus manifest. The corpus
//! cannot be queried until a successful re-run lands a manifest, which is
//! the contract documented in the design spec.
//!
//! Requires a real `llama-server` and both GGUFs (auto-downloaded). Gated
//! behind `FASTRAG_LLAMA_TEST=1` and `#[ignore]`.

#![cfg(all(feature = "contextual", feature = "contextual-llama"))]

mod support;

use std::path::PathBuf;

use assert_cmd::Command;
use tempfile::tempdir;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/contextual_corpus")
}

#[test]
#[ignore]
fn strict_mode_aborts_on_first_failure_no_manifest_written() {
    if std::env::var("FASTRAG_LLAMA_TEST").as_deref() != Ok("1") {
        eprintln!("skipping: set FASTRAG_LLAMA_TEST=1 to run");
        return;
    }
    let Some(model_path) = support::llama_cpp_embed_model_path() else {
        eprintln!(
            "skipping: set FASTRAG_LLAMA_EMBED_MODEL_PATH=/path/to/Qwen3-Embedding-0.6B-Q8_0.gguf"
        );
        return;
    };

    let corpus = tempdir().unwrap();
    let cfg = tempdir().unwrap();
    let config_path = support::write_llama_cpp_config(cfg.path(), "qwen3", &model_path);

    let out = Command::cargo_bin("fastrag")
        .unwrap()
        .args([
            "index",
            fixture_dir().to_str().unwrap(),
            "--corpus",
            corpus.path().to_str().unwrap(),
            "--config",
            config_path.to_str().unwrap(),
            "--contextualize",
            "--context-strict",
        ])
        .env("FASTRAG_TEST_INJECT_FAILURES", "1")
        .output()
        .unwrap();

    assert!(
        !out.status.success(),
        "strict mode should exit non-zero on injected failure, stdout: {} stderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(
        !corpus.path().join("manifest.json").exists(),
        "strict abort should not leave a manifest at {}",
        corpus.path().display()
    );
}
