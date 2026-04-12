//! Shared building blocks for HTTP-backed embedders.
//!
//! Each backend uses a blocking `reqwest::Client`. We keep the corpus indexing
//! path synchronous, so an async runtime is never spun up just for embedding.

use std::time::Duration;

use reqwest::blocking::{Client, RequestBuilder, Response};

use crate::EmbedError;

pub mod ollama;
pub mod openai;

/// Build a blocking reqwest client with sane timeouts for embedding APIs.
pub fn build_client() -> Result<Client, EmbedError> {
    build_client_with_timeout(Duration::from_secs(60))
}

/// Build a blocking reqwest client with a caller-specified request timeout.
///
/// Use this for backends where a single request may take longer than the
/// default 60 s (e.g. CPU-bound reranking of many document pairs).
pub fn build_client_with_timeout(request_timeout: Duration) -> Result<Client, EmbedError> {
    Client::builder()
        .timeout(request_timeout)
        .connect_timeout(Duration::from_secs(10))
        // Disable idle connection pooling. After ~80-90 sequential
        // requests to the same localhost llama-server, hyper's pool can
        // accumulate a stale socket. This reduces the window; the retry
        // logic in send_with_retry handles any remaining failures by
        // falling back to a fresh Client.
        .pool_max_idle_per_host(0)
        .build()
        .map_err(|e| EmbedError::Http(e.to_string()))
}

/// Maximum number of send attempts before giving up.
const MAX_SEND_ATTEMPTS: usize = 6;

/// Send a request with retries on connection errors or 5xx responses.
/// Uses exponential backoff: 1s, 2s, 4s, 8s, 16s between attempts.
///
/// Each `.send()` is executed on a disposable thread with a 90-second
/// deadline.  If reqwest's internal tokio runtime is stuck (a known
/// failure mode after ~80-90 sequential blocking requests), the thread
/// is abandoned and the attempt counts as a transport error — the retry
/// loop continues and callers can still fall back to curl.
///
/// After exhausting all attempts, returns an error. Callers that need a
/// last-resort fallback (e.g. shelling out to curl) can handle the error
/// and try an independent path.
pub fn send_with_retry(make: impl Fn() -> RequestBuilder) -> Result<Response, EmbedError> {
    let mut last_err = None;
    let mut last_5xx: Option<Response> = None;
    for attempt in 0..MAX_SEND_ATTEMPTS {
        if attempt > 0 {
            let backoff_secs = (1u64 << (attempt - 1)).min(16);
            let backoff = Duration::from_secs(backoff_secs);
            eprintln!(
                "[http] attempt {}/{} failed: {}; retrying in {backoff:?}…",
                attempt,
                MAX_SEND_ATTEMPTS,
                last_err.as_deref().unwrap_or("5xx"),
            );
            std::thread::sleep(backoff);
        }
        // Run .send() on a disposable thread so a hung tokio runtime
        // doesn't block the retry loop forever.
        let rb = make();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(rb.send());
        });
        match rx.recv_timeout(Duration::from_secs(90)) {
            Ok(Ok(resp)) if resp.status().is_server_error() => {
                last_err = Some(format!("HTTP {}", resp.status()));
                last_5xx = Some(resp);
            }
            Ok(Ok(resp)) => return Ok(resp),
            Ok(Err(e)) => {
                last_err = Some(format!("{e:#}"));
            }
            Err(_timeout) => {
                eprintln!(
                    "[http] attempt {}/{MAX_SEND_ATTEMPTS}: send() hung, abandoning thread",
                    attempt + 1
                );
                last_err = Some("send() timed out (reqwest runtime stuck)".into());
            }
        }
    }
    // If every attempt got a server response (5xx), return the last one
    // so callers can extract the status code and body via ensure_success.
    // Only return Err for true transport failures (connection refused,
    // hung runtime, etc.) — those are what the curl fallback is for.
    if let Some(resp) = last_5xx {
        return Ok(resp);
    }
    let msg = last_err.unwrap_or_else(|| "unknown".into());
    eprintln!("[http] all {MAX_SEND_ATTEMPTS} attempts failed: {msg}");
    Err(EmbedError::Http(msg))
}

/// Read a response, returning an `Api` error if the status is not 2xx.
pub fn ensure_success(resp: Response) -> Result<Response, EmbedError> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let code = status.as_u16();
    let body = resp.text().unwrap_or_default();
    let mut message = body;
    if message.len() > 500 {
        message.truncate(500);
    }
    Err(EmbedError::Api {
        status: code,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_client_succeeds() {
        let _c = build_client().expect("client builds");
    }
}
