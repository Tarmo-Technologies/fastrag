//! End-to-end CLI test: `fastrag index --cwe-field cwe_id` writes the
//! manifest, `fastrag query --cwe-expand` returns expanded hits.
#![cfg(all(feature = "retrieval", feature = "store"))]

mod support;

use std::fs;
use std::process::Command;

fn bin() -> String {
    env!("CARGO_BIN_EXE_fastrag").to_string()
}

#[test]
fn cli_index_sets_cwe_field_and_query_expands() {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    let jsonl = tmp.path().join("f.jsonl");
    let config_path = support::write_openai_config(
        tmp.path(),
        "openai",
        &[("openai", "text-embedding-3-small")],
    );
    let (openai_base_url, _guard) = support::start_openai_embedding_server();
    fs::write(
        &jsonl,
        r#"{"id":"A","title":"sqli in login","cwe_id":89}
{"id":"B","title":"hibernate hql injection","cwe_id":564}
{"id":"C","title":"stored xss","cwe_id":79}
"#,
    )
    .unwrap();

    let status = Command::new(bin())
        .env("OPENAI_API_KEY", "test")
        .args([
            "index",
            jsonl.to_str().unwrap(),
            "--corpus",
            corpus.to_str().unwrap(),
            "--config",
            config_path.to_str().unwrap(),
            "--openai-base-url",
            &openai_base_url,
            "--format",
            "jsonl",
            "--text-fields",
            "title",
            "--id-field",
            "id",
            "--metadata-fields",
            "cwe_id",
            "--metadata-types",
            "cwe_id=numeric",
            "--cwe-field",
            "cwe_id",
        ])
        .status()
        .unwrap();
    assert!(status.success(), "index failed");

    // Manifest sanity.
    let mbytes = fs::read(corpus.join("manifest.json")).unwrap();
    let mtext = String::from_utf8_lossy(&mbytes);
    let manifest: serde_json::Value = serde_json::from_slice(&mbytes).unwrap();
    assert_eq!(
        manifest.get("cwe_field").and_then(|v| v.as_str()),
        Some("cwe_id"),
        "manifest missing cwe_field: {mtext}"
    );
    assert!(
        mtext.contains("cwe_taxonomy_version"),
        "manifest missing cwe_taxonomy_version"
    );

    // Query with --cwe-expand and --filter cwe_id=89 — no-rerank avoids
    // needing an ONNX model on the test runner.
    let out = Command::new(bin())
        .env("OPENAI_API_KEY", "test")
        .args([
            "query",
            "sqli",
            "--corpus",
            corpus.to_str().unwrap(),
            "--config",
            config_path.to_str().unwrap(),
            "--openai-base-url",
            &openai_base_url,
            "--top-k",
            "10",
            "--filter",
            "cwe_id=89",
            "--cwe-expand",
            "--no-rerank",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "query failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let ids = extract_ids(&stdout);
    assert!(ids.contains("A"), "result missing A: {stdout}");
    assert!(
        ids.contains("B"),
        "result missing B (child CWE 564): {stdout}"
    );
    assert!(!ids.contains("C"), "result should not contain C: {stdout}");

    // Flip with --no-cwe-expand: only A should match.
    let out = Command::new(bin())
        .env("OPENAI_API_KEY", "test")
        .args([
            "query",
            "sqli",
            "--corpus",
            corpus.to_str().unwrap(),
            "--config",
            config_path.to_str().unwrap(),
            "--openai-base-url",
            &openai_base_url,
            "--top-k",
            "10",
            "--filter",
            "cwe_id=89",
            "--no-cwe-expand",
            "--no-rerank",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "query failed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let ids = extract_ids(&stdout);
    assert!(ids.contains("A"), "result missing A: {stdout}");
    assert!(
        !ids.contains("B"),
        "child CWE must not match with --no-cwe-expand: {stdout}"
    );
}

fn extract_ids(json_text: &str) -> std::collections::HashSet<String> {
    let v: serde_json::Value = serde_json::from_str(json_text).unwrap_or(serde_json::Value::Null);
    v.as_array()
        .map(|hits| {
            hits.iter()
                .filter_map(|h| {
                    h.get("source")
                        .and_then(|s| s.get("id"))
                        .and_then(|v| v.as_str())
                        .map(String::from)
                })
                .collect()
        })
        .unwrap_or_default()
}
