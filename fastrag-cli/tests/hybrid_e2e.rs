//! End-to-end: `fastrag query --hybrid` exits 0 with a non-empty JSON hit
//! array. Uses the jsonl ingest path (mirrors cwe_expand_e2e.rs) so we get a
//! typed store with BM25 + dense + metadata — the hybrid path requires both
//! retrievers wired up.
#![cfg(all(feature = "retrieval", feature = "store"))]

use std::fs;
use std::process::Command;

fn bin() -> String {
    env!("CARGO_BIN_EXE_fastrag").to_string()
}

#[test]
fn cli_query_hybrid_returns_results() {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    let jsonl = tmp.path().join("f.jsonl");
    fs::write(
        &jsonl,
        r#"{"id":"A","text":"alpha beta gamma"}
{"id":"B","text":"delta epsilon alpha"}
{"id":"C","text":"alpha delta eta"}
"#,
    )
    .unwrap();

    // Index via jsonl format so the store has both BM25 index and dense HNSW.
    let status = Command::new(bin())
        .args([
            "index",
            jsonl.to_str().unwrap(),
            "--corpus",
            corpus.to_str().unwrap(),
            "--format",
            "jsonl",
            "--text-fields",
            "text",
            "--id-field",
            "id",
        ])
        .status()
        .unwrap();
    assert!(status.success(), "index failed");

    // Query with --hybrid. Assert exit 0 + non-empty JSON array.
    let out = Command::new(bin())
        .args([
            "query",
            "alpha",
            "--corpus",
            corpus.to_str().unwrap(),
            "--top-k",
            "3",
            "--hybrid",
            "--no-rerank",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "query failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let ids = extract_ids(&stdout);
    assert!(
        !ids.is_empty(),
        "expected non-empty hits array; got {stdout}"
    );
    // All three docs contain "alpha" → all three should be retrievable under hybrid.
    assert!(ids.contains("A"), "missing A: {stdout}");
    assert!(ids.contains("C"), "missing C: {stdout}");
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
