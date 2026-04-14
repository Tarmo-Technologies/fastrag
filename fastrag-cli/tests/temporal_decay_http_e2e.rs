//! HTTP e2e: GET /query with hybrid + time_decay query-string params.
#![cfg(all(feature = "retrieval", feature = "store"))]

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use chrono::{Duration, Utc};
use fastrag::ChunkingStrategy;
use fastrag::corpus::CorpusRegistry;
use fastrag::ingest::engine::index_jsonl;
use fastrag::ingest::jsonl::JsonlIngestConfig;
use fastrag_cli::http::{HttpRerankerConfig, serve_http_with_registry};
use fastrag_embed::DynEmbedderTrait;
use fastrag_embed::test_utils::MockEmbedder;
use fastrag_store::schema::TypedKind;

async fn spawn(registry: CorpusRegistry) -> std::net::SocketAddr {
    let embedder: fastrag::DynEmbedder = Arc::new(MockEmbedder);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        serve_http_with_registry(
            registry,
            listener,
            embedder,
            None,  // no token
            false, // dense_only
            false, // cwe_expand_default
            HttpRerankerConfig::default(),
            100,
            None,
            52_428_800,
        )
        .await
        .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    addr
}

fn build_dated_corpus(path: &std::path::Path) {
    let fresh = (Utc::now() - Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let jsonl = path.join("docs.jsonl");
    std::fs::write(
        &jsonl,
        format!(
            r#"{{"id":"STALE","text":"openssl heap overflow","published_date":"2016-01-01"}}
{{"id":"FRESH","text":"openssl heap overflow","published_date":"{fresh}"}}"#
        ),
    )
    .unwrap();

    let corpus = path.join("corpus");
    let embedder = MockEmbedder;
    let cfg = JsonlIngestConfig {
        text_fields: vec!["text".into()],
        id_field: "id".into(),
        metadata_fields: vec!["published_date".into()],
        metadata_types: BTreeMap::from([("published_date".into(), TypedKind::Date)]),
        array_fields: vec![],
        cwe_field: None,
    };
    let chunking = ChunkingStrategy::Basic {
        max_characters: 500,
        overlap: 0,
    };
    index_jsonl(
        &jsonl,
        &corpus,
        &chunking,
        &embedder as &dyn DynEmbedderTrait,
        &cfg,
    )
    .unwrap();
}

fn hit_ids(hits: &[serde_json::Value]) -> HashSet<String> {
    hits.iter()
        .filter_map(|h| {
            h.get("source")
                .and_then(|s| s.get("id"))
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect()
}

fn top_id(hits: &[serde_json::Value]) -> Option<String> {
    hits.first()
        .and_then(|h| h.get("source"))
        .and_then(|s| s.get("id"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

#[tokio::test]
async fn http_query_with_time_decay_promotes_fresh() {
    let tmp = tempfile::tempdir().unwrap();
    build_dated_corpus(tmp.path());

    let registry = CorpusRegistry::new();
    registry.register("default", tmp.path().join("corpus"));
    let addr = spawn(registry).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "http://{addr}/query?q=openssl%20heap%20overflow&top_k=2&hybrid=true\
             &time_decay_field=published_date&time_decay_halflife=30d"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let hits: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(hits.len() >= 2, "expected >=2 hits, got {}", hits.len());
    let ids = hit_ids(&hits);
    assert!(ids.contains("FRESH"), "missing FRESH: {ids:?}");
    assert!(ids.contains("STALE"), "missing STALE: {ids:?}");
    assert_eq!(
        top_id(&hits).as_deref(),
        Some("FRESH"),
        "decay should promote FRESH to top; hits={hits:#?}"
    );
}

#[tokio::test]
async fn http_query_with_bad_blend_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    build_dated_corpus(tmp.path());

    let registry = CorpusRegistry::new();
    registry.register("default", tmp.path().join("corpus"));
    let addr = spawn(registry).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "http://{addr}/query?q=x&top_k=1&time_decay_field=published_date\
             &time_decay_blend=bogus"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400, "expected 400 for bad blend");
}
