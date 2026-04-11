//! llama.cpp HTTP contextualizer backend.
//!
//! Wraps a [`LlamaCppChatClient`] and exposes it through the
//! [`Contextualizer`] trait. All HTTP / parsing errors are mapped onto
//! [`ContextError`] variants so the cache layer and ingest pipeline see a
//! single, crate-local error surface.

use crate::ContextError;
use crate::contextualizer::{Contextualizer, ContextualizerMeta};
use crate::prompt::{PROMPT_VERSION, format_prompt};
use fastrag_embed::llama_cpp::{CompletionError, LlamaCppChatClient};

/// Contextualizer backed by a running `llama-server` completion endpoint.
///
/// Construct via [`Self::new`] once the `llama-server` subprocess is healthy
/// (e.g. through [`fastrag_embed::llama_cpp::LlamaServerPool`]). The
/// contextualizer owns its chat client; dropping it does not tear down the
/// server — the pool handles lifecycle.
pub struct LlamaCppContextualizer {
    client: LlamaCppChatClient,
    model_id: String,
    prompt_version: u32,
}

impl LlamaCppContextualizer {
    /// Build a contextualizer from an already-connected chat client and a
    /// caller-provided model identifier. `model_id` is what ends up in the
    /// SQLite cache primary key, so it must uniquely identify the weights
    /// and quantization — e.g. the [`fastrag_embed::llama_cpp::DefaultCompletionPreset::MODEL_ID`]
    /// constant, or a user-supplied override from `--context-model`.
    pub fn new(client: LlamaCppChatClient, model_id: impl Into<String>) -> Self {
        Self {
            client,
            model_id: model_id.into(),
            prompt_version: PROMPT_VERSION,
        }
    }
}

impl ContextualizerMeta for LlamaCppContextualizer {
    fn model_id(&self) -> &str {
        &self.model_id
    }
    fn prompt_version(&self) -> u32 {
        self.prompt_version
    }
}

/// Test-only counter for the `FASTRAG_TEST_INJECT_FAILURES` env-var hook.
/// Lives at module scope so failure injection is shared across multiple
/// contextualizer instances within a single test process.
static FAIL_INJECTION_COUNT: std::sync::Mutex<usize> = std::sync::Mutex::new(0);

impl Contextualizer for LlamaCppContextualizer {
    fn contextualize(&self, doc_title: &str, raw_chunk: &str) -> Result<String, ContextError> {
        // Test-only injection hook. Reads `FASTRAG_TEST_INJECT_FAILURES=N`
        // and returns `EmptyCompletion` for the first N calls so the
        // retry-failed E2E test can simulate transient backend failures
        // without bringing the real model down. The counter is process-wide
        // so it survives a `--retry-failed` re-invocation that re-uses the
        // same env var (the test unsets it for the retry pass).
        let prompt = format_prompt(doc_title, raw_chunk);
        if let Ok(n_str) = std::env::var("FASTRAG_TEST_INJECT_FAILURES")
            && let Ok(n) = n_str.parse::<usize>()
        {
            let mut count = FAIL_INJECTION_COUNT.lock().unwrap();
            if *count < n {
                *count += 1;
                return Err(ContextError::EmptyCompletion);
            }
        }
        match self.client.complete(&prompt) {
            Ok(text) => Ok(text),
            Err(CompletionError::Http(e)) => {
                // Reqwest surfaces connect/body timeouts via `is_timeout()`.
                // Everything else stays as a generic Http error carrying the
                // stringified cause — we do not re-export reqwest types from
                // fastrag-context to avoid coupling to a specific version.
                if e.is_timeout() {
                    Err(ContextError::Timeout(std::time::Duration::from_secs(60)))
                } else {
                    Err(ContextError::Http(e.to_string()))
                }
            }
            Err(CompletionError::BadStatus { status, body }) => {
                Err(ContextError::BadStatus { status, body })
            }
            Err(CompletionError::EmptyCompletion) => Err(ContextError::EmptyCompletion),
            Err(CompletionError::ParseError(msg)) => Err(ContextError::Parse(msg)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Wrap a blocking `contextualize` call on a plain `std::thread` so the
    /// `reqwest::blocking` client's internal tokio runtime is never dropped
    /// inside our outer tokio runtime (which would otherwise panic with
    /// "Cannot drop a runtime in a context where blocking is not allowed").
    fn run_blocking<F, T>(f: F) -> T
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        std::thread::spawn(f)
            .join()
            .expect("worker thread panicked")
    }

    /// Call the contextualizer from a plain OS thread so the embedded
    /// `reqwest::blocking` runtime is built AND dropped entirely outside
    /// tokio's async context.
    fn run_blocking_call(
        url: String,
        doc_title: &'static str,
        chunk: &'static str,
    ) -> Result<String, ContextError> {
        run_blocking(move || {
            let client = LlamaCppChatClient::new(url, "test-model");
            let ctx = LlamaCppContextualizer::new(client, "test-model");
            ctx.contextualize(doc_title, chunk)
        })
    }

    #[tokio::test]
    async fn success_returns_trimmed_content() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [
                    { "message": { "content": "  This is the context. \n" } }
                ]
            })))
            .mount(&server)
            .await;

        let result = run_blocking_call(server.uri(), "Doc", "A chunk.");
        assert_eq!(result.unwrap(), "This is the context.");
    }

    #[tokio::test]
    async fn http_500_becomes_bad_status() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(500).set_body_string("model OOM"))
            .mount(&server)
            .await;

        let err = run_blocking_call(server.uri(), "Doc", "Chunk.").unwrap_err();

        match err {
            ContextError::BadStatus { status, body } => {
                assert_eq!(status, 500);
                assert!(
                    body.contains("model OOM"),
                    "body should include the server message, got: {body}"
                );
            }
            other => panic!("expected BadStatus, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn empty_content_becomes_empty_completion() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [
                    { "message": { "content": "   " } }
                ]
            })))
            .mount(&server)
            .await;

        let err = run_blocking_call(server.uri(), "Doc", "Chunk.").unwrap_err();

        assert!(
            matches!(err, ContextError::EmptyCompletion),
            "expected EmptyCompletion, got {err:?}"
        );
    }

    #[tokio::test]
    async fn malformed_response_becomes_parse_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let err = run_blocking_call(server.uri(), "Doc", "Chunk.").unwrap_err();

        assert!(
            matches!(err, ContextError::Parse(_)),
            "expected Parse, got {err:?}"
        );
    }

    #[tokio::test]
    async fn missing_choices_field_becomes_parse_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "unexpected": "shape"
            })))
            .mount(&server)
            .await;

        let err = run_blocking_call(server.uri(), "Doc", "Chunk.").unwrap_err();

        assert!(
            matches!(err, ContextError::Parse(_)),
            "expected Parse, got {err:?}"
        );
    }
}
