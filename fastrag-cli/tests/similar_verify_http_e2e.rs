//! End-to-end tests for POST /similar with the optional `verify` block.
#![cfg(feature = "retrieval")]

use std::collections::BTreeMap;
use std::sync::Arc;

use fastrag::ChunkingStrategy;
use fastrag::corpus::CorpusRegistry;
use fastrag::ingest::engine::index_jsonl;
use fastrag::ingest::jsonl::JsonlIngestConfig;
use fastrag_cli::http::{HttpRerankerConfig, serve_http_with_registry};
use fastrag_embed::test_utils::MockEmbedder;
use reqwest::Client;
use reqwest::StatusCode;
use serde_json::json;

fn build_toy_corpus(docs: &[(&str, &str)]) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let jsonl = tmp.path().join("docs.jsonl");
    let lines: Vec<String> = docs
        .iter()
        .map(|(id, body)| json!({"id": id, "body": body}).to_string())
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
        &MockEmbedder as &dyn fastrag::DynEmbedderTrait,
        &cfg,
    )
    .unwrap();
    let holder = tempfile::tempdir().unwrap();
    let dest = holder.path().join("corpus");
    std::fs::rename(&corpus, &dest).unwrap();
    holder
}

async fn spawn_server(registry: CorpusRegistry) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let embedder: Arc<dyn fastrag::DynEmbedderTrait> = Arc::new(MockEmbedder);
    tokio::spawn(async move {
        serve_http_with_registry(
            registry,
            listener,
            embedder,
            None,
            false,
            false,
            HttpRerankerConfig::default(),
            100,
            None,
            52_428_800,
            10_000,
        )
        .await
        .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    format!("http://{}", addr)
}

/// verify block runs Jaccard filter after cosine: near-dup survives,
/// loosely-overlapping chunk is dropped, and verify_score appears on hits.
#[tokio::test]
async fn verify_happy_path_drops_non_dupes_and_exposes_score() {
    let corpus = build_toy_corpus(&[
        ("a", "alpha"),
        (
            "b",
            "alpha zzz qqq xxx yyy completely different tail content here",
        ),
    ]);
    let registry = CorpusRegistry::new();
    registry.register("default", corpus.path().join("corpus"));
    let base = spawn_server(registry).await;

    let resp = Client::new()
        .post(format!("{base}/similar"))
        .json(&json!({
            "text": "alpha",
            "threshold": 0.3,
            "max_results": 10,
            "verify": { "method": "minhash", "threshold": 0.7 }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let hits = body["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1, "only near-dup should survive the verifier");
    let vs = hits[0]["verify_score"]
        .as_f64()
        .expect("verify_score must serialize when verify ran");
    assert!(vs >= 0.7, "surviving hit must be above the Jaccard floor");
    assert_eq!(body["stats"]["dropped_by_verifier"].as_u64().unwrap(), 1);
}

/// Without a `verify` block the response shape is unchanged —
/// verify_score must be absent (skip_serializing_if) and
/// dropped_by_verifier must be absent (skip_serializing_if = zero).
#[tokio::test]
async fn no_verify_produces_backward_compatible_shape() {
    let corpus = build_toy_corpus(&[("a", "alpha"), ("b", "alpha other words")]);
    let registry = CorpusRegistry::new();
    registry.register("default", corpus.path().join("corpus"));
    let base = spawn_server(registry).await;

    let resp = Client::new()
        .post(format!("{base}/similar"))
        .json(&json!({
            "text": "alpha",
            "threshold": 0.3,
            "max_results": 10
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let hits = body["hits"].as_array().unwrap();
    assert!(!hits.is_empty());
    for h in hits {
        assert!(
            h.get("verify_score").is_none(),
            "verify_score must not appear when verify was absent"
        );
    }
    assert!(
        body["stats"].get("dropped_by_verifier").is_none(),
        "dropped_by_verifier must not appear when zero"
    );
}

#[tokio::test]
async fn verify_unknown_method_is_400() {
    let corpus = build_toy_corpus(&[("a", "alpha")]);
    let registry = CorpusRegistry::new();
    registry.register("default", corpus.path().join("corpus"));
    let base = spawn_server(registry).await;

    let resp = Client::new()
        .post(format!("{base}/similar"))
        .json(&json!({
            "text": "alpha",
            "threshold": 0.3,
            "max_results": 10,
            "verify": { "method": "simhash", "threshold": 0.7 }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let text = resp.text().await.unwrap();
    assert!(text.contains("simhash"), "error should name the bad method");
}

#[tokio::test]
async fn verify_method_missing_is_400() {
    let corpus = build_toy_corpus(&[("a", "alpha")]);
    let registry = CorpusRegistry::new();
    registry.register("default", corpus.path().join("corpus"));
    let base = spawn_server(registry).await;

    let resp = Client::new()
        .post(format!("{base}/similar"))
        .json(&json!({
            "text": "alpha",
            "threshold": 0.3,
            "max_results": 10,
            "verify": { "threshold": 0.7 }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let text = resp.text().await.unwrap();
    assert!(text.contains("method"));
}

#[tokio::test]
async fn verify_threshold_out_of_range_is_400() {
    let corpus = build_toy_corpus(&[("a", "alpha")]);
    let registry = CorpusRegistry::new();
    registry.register("default", corpus.path().join("corpus"));
    let base = spawn_server(registry).await;

    let resp = Client::new()
        .post(format!("{base}/similar"))
        .json(&json!({
            "text": "alpha",
            "threshold": 0.3,
            "max_results": 10,
            "verify": { "method": "minhash", "threshold": 1.5 }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let text = resp.text().await.unwrap();
    assert!(text.contains("threshold"));
}

#[tokio::test]
async fn verify_threshold_non_numeric_is_400() {
    let corpus = build_toy_corpus(&[("a", "alpha")]);
    let registry = CorpusRegistry::new();
    registry.register("default", corpus.path().join("corpus"));
    let base = spawn_server(registry).await;

    let resp = Client::new()
        .post(format!("{base}/similar"))
        .json(&json!({
            "text": "alpha",
            "threshold": 0.3,
            "max_results": 10,
            "verify": { "method": "minhash", "threshold": "high" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let text = resp.text().await.unwrap();
    assert!(text.contains("threshold"));
}

#[tokio::test]
async fn verify_unknown_field_is_400() {
    let corpus = build_toy_corpus(&[("a", "alpha")]);
    let registry = CorpusRegistry::new();
    registry.register("default", corpus.path().join("corpus"));
    let base = spawn_server(registry).await;

    let resp = Client::new()
        .post(format!("{base}/similar"))
        .json(&json!({
            "text": "alpha",
            "threshold": 0.3,
            "max_results": 10,
            "verify": { "method": "minhash", "threshold": 0.7, "bogus": true }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let text = resp.text().await.unwrap();
    assert!(text.contains("bogus"));
}
