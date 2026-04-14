//! HTTP e2e: verify `/query?cwe_expand=true` expands CWE-89 to include
//! child CWE-564-tagged documents and the server-wide default flag works.
#![cfg(all(feature = "retrieval", feature = "store"))]

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use fastrag::ChunkingStrategy;
use fastrag::corpus::CorpusRegistry;
use fastrag::ingest::engine::index_jsonl;
use fastrag::ingest::jsonl::JsonlIngestConfig;
use fastrag_cli::http::{HttpRerankerConfig, serve_http_with_registry};
use fastrag_embed::DynEmbedderTrait;
use fastrag_embed::test_utils::MockEmbedder;
use fastrag_store::schema::TypedKind;

async fn spawn_with_default(
    registry: CorpusRegistry,
    cwe_expand_default: bool,
) -> std::net::SocketAddr {
    let embedder: fastrag::DynEmbedder = Arc::new(MockEmbedder);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        serve_http_with_registry(
            registry,
            listener,
            embedder,
            None,
            false,
            cwe_expand_default,
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

fn build_corpus(path: &std::path::Path) {
    let jsonl = path.join("findings.jsonl");
    std::fs::write(
        &jsonl,
        r#"{"id":"A","title":"sqli in login","cwe_id":89}
{"id":"B","title":"hibernate hql injection","cwe_id":564}
{"id":"C","title":"stored xss","cwe_id":79}"#,
    )
    .unwrap();

    let corpus = path.join("corpus");
    let embedder = MockEmbedder;
    let cfg = JsonlIngestConfig {
        text_fields: vec!["title".into()],
        id_field: "id".into(),
        metadata_fields: vec!["cwe_id".into()],
        metadata_types: BTreeMap::from([("cwe_id".into(), TypedKind::Numeric)]),
        array_fields: vec![],
        cwe_field: Some("cwe_id".into()),
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

#[tokio::test]
async fn query_cwe_expand_true_returns_child_cwe_docs() {
    let tmp = tempfile::tempdir().unwrap();
    build_corpus(tmp.path());

    let registry = CorpusRegistry::new();
    registry.register("default", tmp.path().join("corpus"));
    let addr = spawn_with_default(registry, false).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "http://{addr}/query?q=sqli&top_k=10&filter=cwe_id=89&cwe_expand=true"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let hits: Vec<serde_json::Value> = resp.json().await.unwrap();
    let ids = hit_ids(&hits);
    assert!(ids.contains("A"), "missing parent A: {ids:?}");
    assert!(ids.contains("B"), "missing child B (CWE-564): {ids:?}");
    assert!(!ids.contains("C"), "unrelated C should not match: {ids:?}");
}

#[tokio::test]
async fn query_cwe_expand_false_excludes_child() {
    let tmp = tempfile::tempdir().unwrap();
    build_corpus(tmp.path());

    let registry = CorpusRegistry::new();
    registry.register("default", tmp.path().join("corpus"));
    // Server default is ON — the request flips it OFF.
    let addr = spawn_with_default(registry, true).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "http://{addr}/query?q=sqli&top_k=10&filter=cwe_id=89&cwe_expand=false"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let hits: Vec<serde_json::Value> = resp.json().await.unwrap();
    let ids = hit_ids(&hits);
    assert!(ids.contains("A"), "missing parent A: {ids:?}");
    assert!(
        !ids.contains("B"),
        "child B must not appear when cwe_expand=false: {ids:?}"
    );
}

#[tokio::test]
async fn server_default_on_triggers_expansion_without_explicit_param() {
    let tmp = tempfile::tempdir().unwrap();
    build_corpus(tmp.path());

    let registry = CorpusRegistry::new();
    registry.register("default", tmp.path().join("corpus"));
    let addr = spawn_with_default(registry, true).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "http://{addr}/query?q=sqli&top_k=10&filter=cwe_id=89"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let hits: Vec<serde_json::Value> = resp.json().await.unwrap();
    let ids = hit_ids(&hits);
    assert!(ids.contains("A"));
    assert!(
        ids.contains("B"),
        "server default cwe_expand=true should expand: {ids:?}"
    );
}
