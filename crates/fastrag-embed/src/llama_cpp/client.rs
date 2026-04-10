//! Blocking HTTP client for llama-server's OpenAI-compatible
//! `/v1/embeddings` endpoint. The response shape is identical to OpenAI's
//! (`{"data":[{"embedding":[f32,...]}, ...]}`), so we parse it the same way
//! and additionally verify the embedding dimension matches what the caller
//! expected at construction time.
//!
//! This client deliberately does not spawn the server — use it in combination
//! with `LlamaServerHandle` (which exposes a `base_url()`).

use serde::Deserialize;
use serde_json::json;

use crate::error::EmbedError;
use crate::http::{build_client, ensure_success, send_with_retry};

/// Minimal HTTP client that talks to a llama-server `/v1/embeddings` endpoint.
#[derive(Debug)]
pub struct LlamaCppClient {
    base_url: String,
    model_name: String,
    expected_dim: usize,
    http: reqwest::blocking::Client,
}

impl LlamaCppClient {
    /// `base_url` must be the scheme + host + port, without a trailing slash
    /// (e.g. `http://127.0.0.1:8080`). `model_name` is sent in the request
    /// body — llama-server requires it and validates against the loaded model
    /// alias (typically the `--model` path). `expected_dim` is asserted against
    /// the first vector in every response.
    pub fn new(
        base_url: impl Into<String>,
        model_name: impl Into<String>,
        expected_dim: usize,
    ) -> Result<Self, EmbedError> {
        Ok(Self {
            base_url: base_url.into(),
            model_name: model_name.into(),
            expected_dim,
            http: build_client()?,
        })
    }

    /// Build a client that reuses an existing `reqwest::blocking::Client`.
    pub fn with_client(
        base_url: impl Into<String>,
        model_name: impl Into<String>,
        expected_dim: usize,
        http: reqwest::blocking::Client,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            model_name: model_name.into(),
            expected_dim,
            http,
        }
    }

    pub fn expected_dim(&self) -> usize {
        self.expected_dim
    }

    pub fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{}/v1/embeddings", self.base_url);
        let body = json!({ "model": self.model_name, "input": texts });

        let resp = send_with_retry(|| self.http.post(&url).json(&body))?;
        let resp = ensure_success(resp)?;
        let parsed: Resp = resp.json().map_err(|e| EmbedError::Http(e.to_string()))?;

        if parsed.data.len() != texts.len() {
            return Err(EmbedError::UnexpectedDim {
                expected: texts.len(),
                got: parsed.data.len(),
            });
        }

        let vecs: Vec<Vec<f32>> = parsed.data.into_iter().map(|r| r.embedding).collect();
        if let Some(first) = vecs.first()
            && first.len() != self.expected_dim
        {
            return Err(EmbedError::UnexpectedDim {
                expected: self.expected_dim,
                got: first.len(),
            });
        }
        Ok(vecs)
    }
}

#[derive(Deserialize)]
struct Resp {
    data: Vec<RespItem>,
}

#[derive(Deserialize)]
struct RespItem {
    embedding: Vec<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn embed_batch_round_trip() {
        let rt = rt();
        let (uri, _guard) = rt.block_on(async {
            let server = MockServer::start().await;
            let body = json!({
                "data": [
                    { "embedding": vec![0.25_f32; 1024] },
                    { "embedding": vec![0.75_f32; 1024] },
                ]
            });
            Mock::given(method("POST"))
                .and(path("/v1/embeddings"))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
            (server.uri(), server)
        });

        let c = LlamaCppClient::new(uri, "test-model", 1024).unwrap();
        let out = c.embed(&["hello", "world"]).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].len(), 1024);
        assert_eq!(out[1].len(), 1024);
        assert!((out[0][0] - 0.25).abs() < 1e-6);
        assert!((out[1][0] - 0.75).abs() < 1e-6);
    }

    #[test]
    fn server_500_returns_api_error() {
        let rt = rt();
        let (uri, _guard) = rt.block_on(async {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/v1/embeddings"))
                .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
                .mount(&server)
                .await;
            (server.uri(), server)
        });
        let c = LlamaCppClient::new(uri, "test-model", 1024).unwrap();
        let err = c.embed(&["a"]).unwrap_err();
        match err {
            EmbedError::Api { status, message } => {
                assert_eq!(status, 500);
                assert!(message.contains("boom"), "got message: {message}");
            }
            other => panic!("expected Api, got {other:?}"),
        }
    }

    #[test]
    fn dim_mismatch_is_reported() {
        let rt = rt();
        let (uri, _guard) = rt.block_on(async {
            let server = MockServer::start().await;
            let body = json!({ "data": [ { "embedding": vec![0.0_f32; 768] } ] });
            Mock::given(method("POST"))
                .and(path("/v1/embeddings"))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
            (server.uri(), server)
        });
        let c = LlamaCppClient::new(uri, "test-model", 1024).unwrap();
        let err = c.embed(&["a"]).unwrap_err();
        assert!(
            matches!(
                err,
                EmbedError::UnexpectedDim {
                    expected: 1024,
                    got: 768
                }
            ),
            "got {err:?}"
        );
    }

    #[test]
    fn batch_length_mismatch_is_reported() {
        let rt = rt();
        let (uri, _guard) = rt.block_on(async {
            let server = MockServer::start().await;
            let body = json!({ "data": [ { "embedding": vec![0.0_f32; 1024] } ] });
            Mock::given(method("POST"))
                .and(path("/v1/embeddings"))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
            (server.uri(), server)
        });
        let c = LlamaCppClient::new(uri, "test-model", 1024).unwrap();
        let err = c.embed(&["a", "b"]).unwrap_err();
        assert!(
            matches!(
                err,
                EmbedError::UnexpectedDim {
                    expected: 2,
                    got: 1
                }
            ),
            "got {err:?}"
        );
    }

    #[test]
    fn connection_refused_is_http_error() {
        // Bind an ephemeral port, drop the listener, then point the client at it.
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let c =
            LlamaCppClient::new(format!("http://127.0.0.1:{port}"), "test-model", 1024).unwrap();
        let err = c.embed(&["a"]).unwrap_err();
        assert!(
            matches!(err, EmbedError::Http(_)),
            "expected Http transport error, got {err:?}"
        );
    }

    #[test]
    fn empty_input_returns_empty() {
        let c = LlamaCppClient::new("http://127.0.0.1:1", "test-model", 1024).unwrap();
        let out = c.embed(&[]).unwrap();
        assert!(out.is_empty());
    }
}
