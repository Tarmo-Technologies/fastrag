use std::fs;
use std::sync::Arc;

use fastrag::ChunkingStrategy;
use fastrag::ops;
use fastrag_cli::http::serve_http_with_embedder;
use fastrag_embed::test_utils::MockEmbedder;
use reqwest::Client;
use reqwest::StatusCode;
use tokio::net::TcpListener;

fn temp_corpus_dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn sample_input_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("alpha.txt"),
        "ALPHA\n\nalpha beta gamma delta.",
    )
    .unwrap();
    fs::write(
        dir.path().join("beta.txt"),
        "BETA\n\nbeta gamma delta epsilon.",
    )
    .unwrap();
    dir
}

#[tokio::test]
async fn http_query_and_health_end_to_end() {
    let input = sample_input_dir();
    let corpus = temp_corpus_dir();
    let stats = ops::index_path(
        input.path(),
        corpus.path(),
        &ChunkingStrategy::Basic {
            max_characters: 1000,
            overlap: 0,
        },
        &MockEmbedder,
    )
    .unwrap();
    assert_eq!(stats.chunk_count, 2);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn({
        let corpus_dir = corpus.path().to_path_buf();
        let embedder = Arc::new(MockEmbedder);
        async move {
            let _ = serve_http_with_embedder(corpus_dir, listener, embedder).await;
        }
    });

    let client = Client::new();
    let health = client
        .get(format!("http://{addr}/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
    let health_body: serde_json::Value = health.json().await.unwrap();
    assert_eq!(health_body["status"], "ok");

    let response = client
        .get(format!(
            "http://{addr}/query?q=alpha%20beta%20gamma%20delta.&top_k=2"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let hits: serde_json::Value = response.json().await.unwrap();
    let arr = hits.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(
        arr[0]["source_path"],
        input.path().join("alpha.txt").display().to_string()
    );
    assert_eq!(arr[0]["chunk_index"], 0);
    assert!(arr[0]["score"].as_f64().unwrap() >= arr[1]["score"].as_f64().unwrap());

    server.abort();
}
