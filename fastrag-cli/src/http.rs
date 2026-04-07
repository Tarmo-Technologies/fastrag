use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use fastrag::corpus::SearchHitDto;
use fastrag::{Embedder, ops};
use serde::Deserialize;
use serde_json::json;
use thiserror::Error;

#[derive(Clone)]
struct AppState {
    corpus_dir: PathBuf,
    embedder: Arc<dyn Embedder>,
}

#[derive(Debug, Deserialize)]
struct QueryParams {
    q: String,
    #[serde(default = "default_top_k")]
    top_k: usize,
}

fn default_top_k() -> usize {
    5
}

#[derive(Debug, Error)]
pub enum HttpError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("corpus error: {0}")]
    Corpus(#[from] fastrag::corpus::CorpusError),
    #[error("embed loader error: {0}")]
    EmbedLoader(#[from] crate::embed_loader::EmbedLoaderError),
    #[error("server error: {0}")]
    Server(String),
}

pub async fn serve_http(
    corpus_dir: PathBuf,
    port: u16,
    model_path: Option<PathBuf>,
) -> Result<(), HttpError> {
    let embedder = crate::embed_loader::load_embedder(model_path)?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    serve_http_with_embedder(corpus_dir, listener, embedder).await
}

pub async fn serve_http_with_embedder(
    corpus_dir: PathBuf,
    listener: tokio::net::TcpListener,
    embedder: Arc<dyn Embedder>,
) -> Result<(), HttpError> {
    let app = Router::new()
        .route("/health", get(health))
        .route("/query", get(query))
        .with_state(AppState {
            corpus_dir,
            embedder,
        });

    axum::serve(listener, app)
        .await
        .map_err(|e| HttpError::Server(e.to_string()))?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

async fn query(
    State(state): State<AppState>,
    Query(params): Query<QueryParams>,
) -> Result<Json<Vec<SearchHitDto>>, Response> {
    let hits = ops::query_corpus(
        &state.corpus_dir,
        &params.q,
        params.top_k,
        state.embedder.as_ref(),
    )
    .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response())?;

    let hits = hits.into_iter().map(SearchHitDto::from).collect::<Vec<_>>();
    Ok(Json(hits))
}
