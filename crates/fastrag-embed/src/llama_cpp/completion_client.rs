//! Model-agnostic llama-server chat/completion HTTP client.
//!
//! Previously lived in `completion_preset.rs` alongside a hardcoded
//! Qwen3-4B-Instruct `DefaultCompletionPreset`. The preset was deleted in the
//! no-Chinese-origin purge; the transport client stays because it is
//! model-agnostic and used by `fastrag-context`'s `LlamaCppContextualizer`.

use reqwest::blocking::Client;
use serde::Deserialize;

/// Errors that can occur while calling a llama-server `/v1/chat/completions`
/// endpoint.
#[derive(Debug, thiserror::Error)]
pub enum CompletionError {
    #[error("HTTP transport error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("non-success status {status}: {body}")]
    BadStatus { status: u16, body: String },

    #[error("server returned an empty completion")]
    EmptyCompletion,

    #[error("failed to parse server response: {0}")]
    ParseError(String),
}

/// Minimal blocking client for `llama-server --chat-template` style endpoints.
pub struct LlamaCppChatClient {
    base_url: String,
    model: String,
    client: Client,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

impl LlamaCppChatClient {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("reqwest blocking client"),
        }
    }

    /// Send a user prompt and return the trimmed completion body.
    pub fn complete(&self, prompt: &str) -> Result<String, CompletionError> {
        let url = format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/'));
        let payload = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "stream": false,
        });
        let resp = self.client.post(&url).json(&payload).send()?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().unwrap_or_default();
            return Err(CompletionError::BadStatus {
                status: status.as_u16(),
                body,
            });
        }
        let parsed: ChatResponse = resp
            .json()
            .map_err(|e| CompletionError::ParseError(e.to_string()))?;
        let content = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| CompletionError::ParseError("no choices in response".into()))?
            .message
            .content;
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Err(CompletionError::EmptyCompletion);
        }
        Ok(trimmed.to_string())
    }
}
