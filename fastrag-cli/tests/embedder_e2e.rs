#![cfg(feature = "retrieval")]

mod support;

use assert_cmd::Command;
use tempfile::tempdir;

#[test]
fn index_and_query_with_openai_backend() {
    let (uri, _guard) = support::start_openai_embedding_server();

    let docs = tempdir().unwrap();
    std::fs::write(docs.path().join("a.txt"), "hello world").unwrap();
    let corpus = tempdir().unwrap();
    let cfg = tempdir().unwrap();
    let config_path = support::write_openai_config(
        cfg.path(),
        "openai-small",
        &[("openai-small", "text-embedding-3-small")],
    );

    Command::cargo_bin("fastrag")
        .unwrap()
        .env("OPENAI_API_KEY", "test")
        .args([
            "index",
            docs.path().to_str().unwrap(),
            "--corpus",
            corpus.path().to_str().unwrap(),
            "--config",
            config_path.to_str().unwrap(),
            "--openai-base-url",
            &uri,
        ])
        .assert()
        .success();

    Command::cargo_bin("fastrag")
        .unwrap()
        .env("OPENAI_API_KEY", "test")
        .args([
            "query",
            "hello",
            "--corpus",
            corpus.path().to_str().unwrap(),
            "--config",
            config_path.to_str().unwrap(),
            "--openai-base-url",
            &uri,
            // The default reranker now lives in a Tarmo-owned private HF
            // repo; the embedder round-trip tests don't exercise reranking,
            // so skip it to avoid a 401 on model download.
            "--no-rerank",
        ])
        .assert()
        .success();

    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(corpus.path().join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(
        manifest["identity"]["model_id"].as_str().unwrap(),
        "openai:text-embedding-3-small"
    );
    assert_eq!(manifest["identity"]["dim"].as_u64().unwrap(), 1536);
    assert_eq!(manifest["version"].as_u64().unwrap(), 5);
}

#[test]
fn query_with_mismatched_embedder_profile_fails() {
    let (uri, _guard) = support::start_openai_embedding_server();

    let docs = tempdir().unwrap();
    std::fs::write(docs.path().join("a.txt"), "hello").unwrap();
    let corpus = tempdir().unwrap();
    let cfg = tempdir().unwrap();
    let config_path = support::write_openai_config(
        cfg.path(),
        "openai-small",
        &[
            ("openai-small", "text-embedding-3-small"),
            ("openai-large", "text-embedding-3-large"),
        ],
    );

    Command::cargo_bin("fastrag")
        .unwrap()
        .env("OPENAI_API_KEY", "test")
        .args([
            "index",
            docs.path().to_str().unwrap(),
            "--corpus",
            corpus.path().to_str().unwrap(),
            "--config",
            config_path.to_str().unwrap(),
            "--openai-base-url",
            &uri,
        ])
        .assert()
        .success();

    let out = Command::cargo_bin("fastrag")
        .unwrap()
        .env("OPENAI_API_KEY", "test")
        .args([
            "query",
            "hello",
            "--corpus",
            corpus.path().to_str().unwrap(),
            "--config",
            config_path.to_str().unwrap(),
            "--embedder-profile",
            "openai-large",
            "--openai-base-url",
            &uri,
            "--no-rerank",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("openai:text-embedding-3-small") && stderr.contains("identity mismatch"),
        "stderr should mention identity mismatch + existing model_id, got: {stderr}"
    );
}
