//! Integration tests for multi-corpus federation.

use fastrag::corpus::CorpusRegistry;
use fastrag_cli::http::{HttpRerankerConfig, serve_http_with_registry};
use fastrag_embed::test_utils::MockEmbedder;
use std::sync::Arc;

fn toy_corpus() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let doc_path = dir.path().join("doc.txt");
    std::fs::write(&doc_path, "SQL injection vulnerability").unwrap();
    fastrag::corpus::index_path(
        &doc_path,
        dir.path(),
        &fastrag::ChunkingStrategy::Basic {
            max_characters: 1000,
            overlap: 0,
        },
        &MockEmbedder,
    )
    .unwrap();
    dir
}

#[tokio::test]
async fn named_corpus_query() {
    let dir = toy_corpus();
    let e = Arc::new(MockEmbedder);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let registry = CorpusRegistry::new();
    registry.register("docs", dir.path().to_path_buf());

    tokio::spawn(async move {
        serve_http_with_registry(
            registry,
            listener,
            e,
            None,
            false,
            HttpRerankerConfig::default(),
            100,
            None,
        )
        .await
        .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let client = reqwest::Client::new();

    // Query named corpus -> 200
    let resp = client
        .get(format!("http://{}/query?q=SQL&corpus=docs&top_k=3", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Query unknown corpus -> 404
    let resp = client
        .get(format!(
            "http://{}/query?q=SQL&corpus=unknown&top_k=3",
            addr
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn get_corpora_lists_registry() {
    let dir = toy_corpus();
    let e = Arc::new(MockEmbedder);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let registry = CorpusRegistry::new();
    registry.register("nvd", dir.path().to_path_buf());

    tokio::spawn(async move {
        serve_http_with_registry(
            registry,
            listener,
            e,
            None,
            false,
            HttpRerankerConfig::default(),
            100,
            None,
        )
        .await
        .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/corpora", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    let corpora = json["corpora"].as_array().unwrap();
    assert_eq!(corpora.len(), 1);
    assert_eq!(corpora[0]["name"].as_str().unwrap(), "nvd");
    assert_eq!(corpora[0]["status"].as_str().unwrap(), "unloaded");
}

#[tokio::test]
async fn default_corpus_used_when_no_corpus_param() {
    let dir = toy_corpus();
    let e = Arc::new(MockEmbedder);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let registry = CorpusRegistry::new();
    registry.register("default", dir.path().to_path_buf());

    tokio::spawn(async move {
        serve_http_with_registry(
            registry,
            listener,
            e,
            None,
            false,
            HttpRerankerConfig::default(),
            100,
            None,
        )
        .await
        .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    // No corpus= param -> uses "default"
    let resp = client
        .get(format!("http://{}/query?q=SQL&top_k=3", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}
