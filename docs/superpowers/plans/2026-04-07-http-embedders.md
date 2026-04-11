# HTTP Embedder Backends (#31a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add OpenAI and Ollama HTTP embedder backends alongside the existing local BGE, wired through `index`, `query`, and `serve-http` with manifest auto-detection.

**Architecture:** New `http` module inside `fastrag-embed` with two `Embedder` impls using blocking `reqwest`, gated behind a new `http-embedders` feature. The `Embedder::model_id` signature changes from `&'static str` to `String` so runtime-built ids like `openai:text-embedding-3-small` can flow into the corpus manifest. CLI dispatches on `--embedder` and auto-detects backend from `embedding_model_id` on read paths. Corpus indexing rejects embedder mismatches against an existing manifest.

**Tech Stack:** Rust, `reqwest` (blocking, json, rustls-tls), `wiremock` (async; used via its blocking helpers in sync tests through `tokio::runtime`), `thiserror`, `serde_json`, `clap`.

**Spec:** `docs/superpowers/specs/2026-04-07-http-embedders-design.md`

---

## File Structure

**Create:**
- `crates/fastrag-embed/src/http/mod.rs` — shared client builder, JSON helpers, `http_post_json` with single 5xx retry
- `crates/fastrag-embed/src/http/openai.rs` — `OpenAIEmbedder` + unit tests
- `crates/fastrag-embed/src/http/ollama.rs` — `OllamaEmbedder` + unit tests
- `crates/fastrag-embed/tests/http_e2e.rs` — integration test of both backends through `Embedder` trait against wiremock
- `fastrag-cli/tests/embedder_e2e.rs` — CLI-level index/query/mismatch tests against wiremock

**Modify:**
- `crates/fastrag-embed/Cargo.toml` — add `http-embedders` feature, optional `reqwest`, dev-dep `wiremock` + `tokio`
- `crates/fastrag-embed/src/lib.rs` — trait signature change, module wiring
- `crates/fastrag-embed/src/bge.rs` — update `model_id()` to return `String`
- `crates/fastrag-embed/src/test_utils.rs` — update `MockEmbedder::model_id()`
- `crates/fastrag-embed/src/error.rs` — add `MissingEnv`, `Http`, `Api`, `DimensionProbeFailed`, `UnknownModel`
- `crates/fastrag/src/corpus/mod.rs` — `EmbedderMismatch` variant + check in `index_path_with_metadata`
- `fastrag-cli/Cargo.toml` — enable `http-embedders` through retrieval; dev-deps wiremock+tokio
- `fastrag-cli/src/args.rs` — new `EmbedderArg` enum + flags on `Index`/`Query`/`ServeHttp`
- `fastrag-cli/src/embed_loader.rs` — dispatch on embedder kind, auto-detect from manifest
- `fastrag-cli/src/main.rs` — wire new args
- `fastrag-cli/src/http.rs` — wire embedder construction from manifest
- `README.md` — document backends, env vars, real-API smoke tests

---

## Task 1: Change `Embedder::model_id` to return `String`

**Files:**
- Modify: `crates/fastrag-embed/src/lib.rs`
- Modify: `crates/fastrag-embed/src/bge.rs`
- Modify: `crates/fastrag-embed/src/test_utils.rs`
- Modify: `crates/fastrag-embed/src/lib.rs` `trait_tests` and `test_utils.rs` existing tests

- [ ] **Step 1: Write the failing test for runtime model_id**

Add to `crates/fastrag-embed/src/test_utils.rs` below `model_id_is_stable`:

```rust
#[test]
fn model_id_returns_owned_string() {
    let e = MockEmbedder::new(16);
    let id: String = e.model_id();
    assert_eq!(id, "fastrag/mock-embedder-16d-v1");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p fastrag-embed --features test-utils model_id_returns_owned_string`
Expected: FAIL — `expected struct String, found &str`.

- [ ] **Step 3: Update trait signature in `crates/fastrag-embed/src/lib.rs`**

Replace lines 11-17:

```rust
pub trait Embedder: Send + Sync {
    /// An identifier for the embedding model implementation used.
    ///
    /// This is written into corpus manifests to enforce compatibility at load time.
    /// Returns an owned `String` so HTTP-backed embedders can encode runtime-chosen
    /// model names (e.g. `"openai:text-embedding-3-small"`).
    fn model_id(&self) -> String {
        "unknown".to_string()
    }
```

- [ ] **Step 4: Update `BgeSmallEmbedder` impl**

In `crates/fastrag-embed/src/bge.rs` around line 118, replace:

```rust
    fn model_id(&self) -> String {
        "fastrag/bge-small-en-v1.5".to_string()
    }
```

- [ ] **Step 5: Update `MockEmbedder` impl**

In `crates/fastrag-embed/src/test_utils.rs` around line 44:

```rust
    fn model_id(&self) -> String {
        format!("fastrag/mock-embedder-{}d-v1", self.dim)
    }
```

And update the existing `model_id_is_stable` test assertion to compare to a `&str` via `.as_str()` if needed:

```rust
#[test]
fn model_id_is_stable() {
    let e = MockEmbedder::new(16);
    assert_eq!(e.model_id(), "fastrag/mock-embedder-16d-v1");
}
```

- [ ] **Step 6: Fix the one call site in the corpus crate**

In `crates/fastrag/src/corpus/mod.rs:145`, the line is already `embedder.model_id().to_string()`. Change to `embedder.model_id()` (the `.to_string()` becomes redundant but is still valid — leave it if it already compiles; the compiler will complain on deprecated-like warnings only, not errors).

No change needed if `.to_string()` on `String` still compiles (it does — `String: ToString`). Leave as is.

- [ ] **Step 7: Run workspace tests**

Run: `cargo test --workspace --features retrieval`
Expected: PASS (all pre-existing tests + new `model_id_returns_owned_string`).

- [ ] **Step 8: Clippy + fmt**

Run: `cargo clippy --workspace --all-targets --features retrieval -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 9: Commit**

```bash
git add crates/fastrag-embed crates/fastrag/src/corpus/mod.rs
git commit -m "refactor(embed): return owned String from Embedder::model_id

Prepares for runtime-composed ids like openai:<model>. No behavior change.

Refs #31a"
```

---

## Task 2: Add new `EmbedError` variants

**Files:**
- Modify: `crates/fastrag-embed/src/error.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/fastrag-embed/src/error.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_variants_format_cleanly() {
        let e = EmbedError::MissingEnv("OPENAI_API_KEY");
        assert_eq!(e.to_string(), "missing required environment variable: OPENAI_API_KEY");

        let e = EmbedError::Http("connection reset".into());
        assert_eq!(e.to_string(), "http transport error: connection reset");

        let e = EmbedError::Api { status: 401, message: "bad key".into() };
        assert_eq!(e.to_string(), "api error: status 401: bad key");

        let e = EmbedError::DimensionProbeFailed("refused".into());
        assert_eq!(e.to_string(), "dimension probe failed: refused");

        let e = EmbedError::UnknownModel {
            backend: "openai",
            model: "text-embedding-9001".into(),
        };
        assert_eq!(e.to_string(), "unknown model for backend openai: text-embedding-9001");
    }
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p fastrag-embed error_variants_format_cleanly`
Expected: FAIL — variants don't exist.

- [ ] **Step 3: Add variants**

In `crates/fastrag-embed/src/error.rs`, add inside the enum before `EmptyInput`:

```rust
    #[error("missing required environment variable: {0}")]
    MissingEnv(&'static str),

    #[error("http transport error: {0}")]
    Http(String),

    #[error("api error: status {status}: {message}")]
    Api { status: u16, message: String },

    #[error("dimension probe failed: {0}")]
    DimensionProbeFailed(String),

    #[error("unknown model for backend {backend}: {model}")]
    UnknownModel { backend: &'static str, model: String },
```

- [ ] **Step 4: Run**

Run: `cargo test -p fastrag-embed`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/fastrag-embed/src/error.rs
git commit -m "feat(embed): add HTTP backend error variants

Adds MissingEnv, Http, Api, DimensionProbeFailed, UnknownModel.

Refs #31a"
```

---

## Task 3: Add `http-embedders` feature + reqwest dep

**Files:**
- Modify: `crates/fastrag-embed/Cargo.toml`

- [ ] **Step 1: Update Cargo.toml**

Replace the `[dependencies]` and `[features]` sections:

```toml
[dependencies]
thiserror.workspace = true
serde.workspace = true

# ML / model loading
candle-core = "0.10.2"
candle-nn = "0.10.2"
candle-transformers = "0.10.2"
tokenizers = "0.22.2"
hf-hub = { version = "0.5.0", default-features = false, features = ["ureq"] }
dirs = "6.0.0"
serde_json = "1"

# HTTP embedders (optional)
reqwest = { version = "0.12", default-features = false, features = ["blocking", "json", "rustls-tls"], optional = true }

[dev-dependencies]
wiremock = "0.6"
tokio = { version = "1", features = ["rt", "macros"] }

[features]
# Exposes a deterministic MockEmbedder for downstream integration tests.
test-utils = []
# Enables OpenAI + Ollama HTTP embedder backends.
http-embedders = ["dep:reqwest"]
```

- [ ] **Step 2: Verify build**

Run: `cargo build -p fastrag-embed --features http-embedders`
Expected: builds cleanly (no source changes yet; reqwest is unused so allow dead_code warning — fine).

Run: `cargo build -p fastrag-embed`
Expected: builds without reqwest.

- [ ] **Step 3: Commit**

```bash
git add crates/fastrag-embed/Cargo.toml
git commit -m "build(embed): add http-embedders feature and reqwest dep

Refs #31a"
```

---

## Task 4: Shared HTTP helpers module

**Files:**
- Create: `crates/fastrag-embed/src/http/mod.rs`
- Modify: `crates/fastrag-embed/src/lib.rs`

- [ ] **Step 1: Wire module into lib**

In `crates/fastrag-embed/src/lib.rs`, after `mod bge;` add:

```rust
#[cfg(feature = "http-embedders")]
pub mod http;
```

- [ ] **Step 2: Write the failing test**

Create `crates/fastrag-embed/src/http/mod.rs`:

```rust
//! Shared building blocks for HTTP-backed embedders.
//!
//! Each backend uses a blocking `reqwest::Client`. We keep the corpus indexing
//! path synchronous, so an async runtime is never spun up just for embedding.

use std::time::Duration;

use reqwest::blocking::{Client, RequestBuilder, Response};

use crate::EmbedError;

pub mod openai;
pub mod ollama;

/// Build a blocking reqwest client with sane timeouts for embedding APIs.
pub(crate) fn build_client() -> Result<Client, EmbedError> {
    Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| EmbedError::Http(e.to_string()))
}

/// Send a request with a single retry on connection errors or 5xx responses.
/// Backoff is a fixed 500ms between the two attempts.
pub(crate) fn send_with_retry(make: impl Fn() -> RequestBuilder) -> Result<Response, EmbedError> {
    match make().send() {
        Ok(resp) if resp.status().is_server_error() => {
            std::thread::sleep(Duration::from_millis(500));
            make().send().map_err(|e| EmbedError::Http(e.to_string()))
        }
        Ok(resp) => Ok(resp),
        Err(_) => {
            std::thread::sleep(Duration::from_millis(500));
            make().send().map_err(|e| EmbedError::Http(e.to_string()))
        }
    }
}

/// Read a response, returning an `Api` error if the status is not 2xx.
pub(crate) fn ensure_success(resp: Response) -> Result<Response, EmbedError> {
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
    Err(EmbedError::Api { status: code, message })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_client_succeeds() {
        let c = build_client().expect("client builds");
        // Smoke: client is constructible; timeouts are set.
        let _ = c;
    }
}
```

- [ ] **Step 3: Run**

Run: `cargo test -p fastrag-embed --features http-embedders build_client_succeeds`
Expected: PASS. Compilation will fail if `openai.rs`/`ollama.rs` don't exist — create empty stubs:

Create `crates/fastrag-embed/src/http/openai.rs`:
```rust
//! OpenAI embedder — filled in by Task 5.
```

Create `crates/fastrag-embed/src/http/ollama.rs`:
```rust
//! Ollama embedder — filled in by Task 6.
```

Re-run: `cargo test -p fastrag-embed --features http-embedders build_client_succeeds`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/fastrag-embed/src/http crates/fastrag-embed/src/lib.rs
git commit -m "feat(embed): add http module scaffolding

Shared blocking reqwest client + retry helper for HTTP embedders.

Refs #31a"
```

---

## Task 5: `OpenAIEmbedder`

**Files:**
- Modify: `crates/fastrag-embed/src/http/openai.rs`

- [ ] **Step 1: Write failing happy-path test**

Replace `crates/fastrag-embed/src/http/openai.rs` with:

```rust
//! OpenAI embedder backend.
//!
//! Blocking HTTP client. Static dim table — no silent probing. Supports
//! OpenAI's native batch input in one request.

use std::env;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::http::{build_client, ensure_success, send_with_retry};
use crate::{EmbedError, Embedder};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

fn dim_for(model: &str) -> Option<usize> {
    match model {
        "text-embedding-3-small" => Some(1536),
        "text-embedding-3-large" => Some(3072),
        _ => None,
    }
}

pub struct OpenAIEmbedder {
    model: String,
    api_key: String,
    base_url: String,
    dim: usize,
    client: reqwest::blocking::Client,
}

impl OpenAIEmbedder {
    pub fn new(model: impl Into<String>) -> Result<Self, EmbedError> {
        let model = model.into();
        let dim = dim_for(&model).ok_or_else(|| EmbedError::UnknownModel {
            backend: "openai",
            model: model.clone(),
        })?;
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| EmbedError::MissingEnv("OPENAI_API_KEY"))?;
        Ok(Self {
            model,
            api_key,
            base_url: DEFAULT_BASE_URL.to_string(),
            dim,
            client: build_client()?,
        })
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }
}

#[derive(Serialize)]
struct Req<'a> {
    model: &'a str,
    input: &'a [&'a str],
}

#[derive(Deserialize)]
struct Resp {
    data: Vec<RespItem>,
}

#[derive(Deserialize)]
struct RespItem {
    embedding: Vec<f32>,
}

impl Embedder for OpenAIEmbedder {
    fn model_id(&self) -> String {
        format!("openai:{}", self.model)
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn default_batch_size(&self) -> usize {
        512
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{}/embeddings", self.base_url);
        let body = json!({ "model": &self.model, "input": texts });
        let resp = send_with_retry(|| {
            self.client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&body)
        })?;
        let resp = ensure_success(resp)?;
        let parsed: Resp = resp.json().map_err(|e| EmbedError::Http(e.to_string()))?;
        if parsed.data.len() != texts.len() {
            return Err(EmbedError::UnexpectedDim {
                expected: texts.len(),
                got: parsed.data.len(),
            });
        }
        let vecs: Vec<Vec<f32>> = parsed.data.into_iter().map(|r| r.embedding).collect();
        if let Some(first) = vecs.first() {
            if first.len() != self.dim {
                return Err(EmbedError::UnexpectedDim {
                    expected: self.dim,
                    got: first.len(),
                });
            }
        }
        Ok(vecs)
    }
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

    fn make_embedder(base: &str) -> OpenAIEmbedder {
        // SAFETY: single-threaded test env var set.
        unsafe { std::env::set_var("OPENAI_API_KEY", "test-key") };
        OpenAIEmbedder::new("text-embedding-3-small")
            .unwrap()
            .with_base_url(base.to_string())
    }

    #[test]
    fn happy_path_round_trip() {
        let rt = rt();
        let (server_uri, _guard) = rt.block_on(async {
            let server = MockServer::start().await;
            let body = json!({
                "data": [
                    { "embedding": vec![0.1_f32; 1536] },
                    { "embedding": vec![0.2_f32; 1536] },
                ]
            });
            Mock::given(method("POST"))
                .and(path("/embeddings"))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
            (server.uri(), server)
        });
        let e = make_embedder(&server_uri);
        let out = e.embed(&["a", "b"]).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].len(), 1536);
        assert!((out[0][0] - 0.1).abs() < 1e-6);
        assert!((out[1][0] - 0.2).abs() < 1e-6);
    }

    #[test]
    fn api_error_401() {
        let rt = rt();
        let (server_uri, _guard) = rt.block_on(async {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/embeddings"))
                .respond_with(
                    ResponseTemplate::new(401).set_body_string(r#"{"error":"bad key"}"#),
                )
                .mount(&server)
                .await;
            (server.uri(), server)
        });
        let e = make_embedder(&server_uri);
        let err = e.embed(&["a"]).unwrap_err();
        match err {
            EmbedError::Api { status, message } => {
                assert_eq!(status, 401);
                assert!(message.contains("bad key"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[test]
    fn length_mismatch() {
        let rt = rt();
        let (server_uri, _guard) = rt.block_on(async {
            let server = MockServer::start().await;
            let body = json!({ "data": [ { "embedding": vec![0.0_f32; 1536] } ] });
            Mock::given(method("POST"))
                .and(path("/embeddings"))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
            (server.uri(), server)
        });
        let e = make_embedder(&server_uri);
        let err = e.embed(&["a", "b"]).unwrap_err();
        matches!(err, EmbedError::UnexpectedDim { expected: 2, got: 1 });
    }

    #[test]
    fn unknown_model_is_rejected() {
        unsafe { std::env::set_var("OPENAI_API_KEY", "k") };
        let err = OpenAIEmbedder::new("text-embedding-9001").unwrap_err();
        match err {
            EmbedError::UnknownModel { backend, model } => {
                assert_eq!(backend, "openai");
                assert_eq!(model, "text-embedding-9001");
            }
            other => panic!("expected UnknownModel, got {other:?}"),
        }
    }

    #[test]
    fn model_id_is_namespaced() {
        unsafe { std::env::set_var("OPENAI_API_KEY", "k") };
        let e = OpenAIEmbedder::new("text-embedding-3-large").unwrap();
        assert_eq!(e.model_id(), "openai:text-embedding-3-large");
        assert_eq!(e.dim(), 3072);
    }
}
```

- [ ] **Step 2: Run**

Run: `cargo test -p fastrag-embed --features http-embedders openai::`
Expected: all five tests PASS.

- [ ] **Step 3: Clippy**

Run: `cargo clippy -p fastrag-embed --features http-embedders --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/fastrag-embed/src/http/openai.rs
git commit -m "feat(embed): add OpenAIEmbedder HTTP backend

Blocking reqwest, static dim table for text-embedding-3-{small,large},
OPENAI_API_KEY env, 512 batch size, one retry on 5xx.

Refs #31a"
```

---

## Task 6: `OllamaEmbedder`

**Files:**
- Modify: `crates/fastrag-embed/src/http/ollama.rs`

- [ ] **Step 1: Write tests + implementation**

Replace `crates/fastrag-embed/src/http/ollama.rs` with:

```rust
//! Ollama embedder backend.
//!
//! Ollama's /api/embeddings endpoint takes a single `prompt` at a time and
//! hosts arbitrary user-pulled models, so we probe dimension on construction.

use std::env;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::http::{build_client, ensure_success, send_with_retry};
use crate::{EmbedError, Embedder};

const DEFAULT_BASE_URL: &str = "http://localhost:11434";

pub struct OllamaEmbedder {
    model: String,
    base_url: String,
    dim: usize,
    client: reqwest::blocking::Client,
}

#[derive(Serialize)]
struct Req<'a> {
    model: &'a str,
    prompt: &'a str,
}

#[derive(Deserialize)]
struct Resp {
    embedding: Vec<f32>,
}

impl OllamaEmbedder {
    pub fn new(model: impl Into<String>) -> Result<Self, EmbedError> {
        let model = model.into();
        let base_url = env::var("OLLAMA_HOST").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        Self::construct(model, base_url)
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        // Re-probe dimension against the new base url.
        match probe_dim(&self.client, &self.base_url, &self.model) {
            Ok(d) => {
                self.dim = d;
                self
            }
            Err(_) => self,
        }
    }

    fn construct(model: String, base_url: String) -> Result<Self, EmbedError> {
        let client = build_client()?;
        let dim = probe_dim(&client, &base_url, &model)?;
        Ok(Self {
            model,
            base_url,
            dim,
            client,
        })
    }
}

fn probe_dim(
    client: &reqwest::blocking::Client,
    base_url: &str,
    model: &str,
) -> Result<usize, EmbedError> {
    let url = format!("{}/api/embeddings", base_url);
    let body = json!({ "model": model, "prompt": "a" });
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .map_err(|e| EmbedError::DimensionProbeFailed(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(EmbedError::DimensionProbeFailed(format!(
            "status {}",
            resp.status().as_u16()
        )));
    }
    let parsed: Resp = resp
        .json()
        .map_err(|e| EmbedError::DimensionProbeFailed(e.to_string()))?;
    if parsed.embedding.is_empty() {
        return Err(EmbedError::DimensionProbeFailed(
            "empty embedding vector".into(),
        ));
    }
    Ok(parsed.embedding.len())
}

impl Embedder for OllamaEmbedder {
    fn model_id(&self) -> String {
        format!("ollama:{}", self.model)
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn default_batch_size(&self) -> usize {
        1
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{}/api/embeddings", self.base_url);
        let mut out = Vec::with_capacity(texts.len());
        for text in texts {
            let body = json!({ "model": &self.model, "prompt": text });
            let resp = send_with_retry(|| self.client.post(&url).json(&body))?;
            let resp = ensure_success(resp)?;
            let parsed: Resp = resp.json().map_err(|e| EmbedError::Http(e.to_string()))?;
            if parsed.embedding.len() != self.dim {
                return Err(EmbedError::UnexpectedDim {
                    expected: self.dim,
                    got: parsed.embedding.len(),
                });
            }
            out.push(parsed.embedding);
        }
        Ok(out)
    }
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

    async fn mount_probe_and_embed(server: &MockServer, dim: usize) {
        let body = json!({ "embedding": vec![0.25_f32; dim] });
        Mock::given(method("POST"))
            .and(path("/api/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }

    #[test]
    fn happy_path() {
        let rt = rt();
        let (uri, _g) = rt.block_on(async {
            let server = MockServer::start().await;
            mount_probe_and_embed(&server, 4).await;
            (server.uri(), server)
        });
        // Point OLLAMA_HOST at the fake so `new` probes against it.
        unsafe { std::env::set_var("OLLAMA_HOST", &uri) };
        let e = OllamaEmbedder::new("nomic-embed-text").unwrap();
        assert_eq!(e.dim(), 4);
        let out = e.embed(&["hello", "world"]).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].len(), 4);
    }

    #[test]
    fn probe_failure_on_refused() {
        unsafe { std::env::set_var("OLLAMA_HOST", "http://127.0.0.1:1") };
        let err = OllamaEmbedder::new("nomic-embed-text").unwrap_err();
        matches!(err, EmbedError::DimensionProbeFailed(_));
    }

    #[test]
    fn missing_model_404() {
        let rt = rt();
        // Probe succeeds (returns 4-d), then the embed call returns 404.
        let (uri, _g) = rt.block_on(async {
            let server = MockServer::start().await;
            // Up to n probes succeed; after that return 404. wiremock matches in insertion order
            // with `.up_to_n_times`.
            Mock::given(method("POST"))
                .and(path("/api/embeddings"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({ "embedding": vec![0.0_f32; 4] })),
                )
                .up_to_n_times(1)
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path("/api/embeddings"))
                .respond_with(ResponseTemplate::new(404).set_body_string("model not found"))
                .mount(&server)
                .await;
            (server.uri(), server)
        });
        unsafe { std::env::set_var("OLLAMA_HOST", &uri) };
        let e = OllamaEmbedder::new("nomic-embed-text").unwrap();
        let err = e.embed(&["hello"]).unwrap_err();
        match err {
            EmbedError::Api { status: 404, .. } => {}
            other => panic!("expected Api 404, got {other:?}"),
        }
    }

    #[test]
    fn model_id_is_namespaced() {
        let rt = rt();
        let (uri, _g) = rt.block_on(async {
            let server = MockServer::start().await;
            mount_probe_and_embed(&server, 8).await;
            (server.uri(), server)
        });
        unsafe { std::env::set_var("OLLAMA_HOST", &uri) };
        let e = OllamaEmbedder::new("nomic-embed-text").unwrap();
        assert_eq!(e.model_id(), "ollama:nomic-embed-text");
        assert_eq!(e.default_batch_size(), 1);
    }
}
```

- [ ] **Step 2: Run**

Run: `cargo test -p fastrag-embed --features http-embedders ollama:: -- --test-threads=1`
(Serial to avoid env-var interference.)
Expected: PASS.

- [ ] **Step 3: Clippy + fmt**

Run: `cargo clippy -p fastrag-embed --features http-embedders --all-targets -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/fastrag-embed/src/http/ollama.rs
git commit -m "feat(embed): add OllamaEmbedder HTTP backend

Blocking reqwest, dim probed on construction, single-prompt API, batch=1.
OLLAMA_HOST env var with http://localhost:11434 default.

Refs #31a"
```

---

## Task 7: Cross-backend integration test

**Files:**
- Create: `crates/fastrag-embed/tests/http_e2e.rs`

- [ ] **Step 1: Write failing test**

Create `crates/fastrag-embed/tests/http_e2e.rs`:

```rust
#![cfg(feature = "http-embedders")]

use fastrag_embed::http::{ollama::OllamaEmbedder, openai::OpenAIEmbedder};
use fastrag_embed::Embedder;
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
fn openai_embed_through_trait() {
    let rt = rt();
    let (uri, _g) = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/embeddings"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({
                    "data": [
                        { "embedding": vec![0.1_f32; 1536] },
                        { "embedding": vec![0.2_f32; 1536] },
                    ]
                })),
            )
            .mount(&server)
            .await;
        (server.uri(), server)
    });
    unsafe { std::env::set_var("OPENAI_API_KEY", "k") };
    let e: Box<dyn Embedder> = Box::new(
        OpenAIEmbedder::new("text-embedding-3-small")
            .unwrap()
            .with_base_url(uri),
    );
    let vecs = e.embed(&["a", "b"]).unwrap();
    assert_eq!(vecs.len(), 2);
    assert_eq!(e.dim(), 1536);
    assert_eq!(e.model_id(), "openai:text-embedding-3-small");
}

#[test]
fn ollama_embed_through_trait() {
    let rt = rt();
    let (uri, _g) = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/embeddings"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "embedding": vec![0.3_f32; 6] })),
            )
            .mount(&server)
            .await;
        (server.uri(), server)
    });
    unsafe { std::env::set_var("OLLAMA_HOST", &uri) };
    let e: Box<dyn Embedder> = Box::new(OllamaEmbedder::new("nomic-embed-text").unwrap());
    let vecs = e.embed(&["a", "b"]).unwrap();
    assert_eq!(vecs.len(), 2);
    assert_eq!(vecs[0].len(), 6);
    assert_eq!(e.model_id(), "ollama:nomic-embed-text");
}
```

Also ensure `crates/fastrag-embed/src/http/mod.rs` declares `pub mod openai;` and `pub mod ollama;` (it does per Task 4).

- [ ] **Step 2: Run**

Run: `cargo test -p fastrag-embed --features http-embedders --test http_e2e -- --test-threads=1`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/fastrag-embed/tests/http_e2e.rs
git commit -m "test(embed): add HTTP backend trait-level e2e tests

Refs #31a"
```

---

## Task 8: `CorpusError::EmbedderMismatch` + index-time check

**Files:**
- Modify: `crates/fastrag/src/corpus/mod.rs`

- [ ] **Step 1: Write failing test**

Append to the existing `#[cfg(test)] mod tests` in `crates/fastrag/src/corpus/mod.rs` (or the test file where corpus tests live — find it with `grep -l "fn.*index_path" crates/fastrag/src/corpus/`). Add a new unit test using `MockEmbedder` with two different dims is not suitable because `MockEmbedder` hard-codes a single id pattern. Use two mocks with different dims to produce different `model_id`s.

Add this test at the end of `crates/fastrag/src/corpus/mod.rs` inside the existing `#[cfg(test)] mod tests { ... }` block (create the block if absent):

```rust
#[cfg(test)]
mod embedder_mismatch_tests {
    use super::*;
    use fastrag_embed::test_utils::MockEmbedder;
    use tempfile::tempdir;

    #[test]
    fn index_rejects_different_embedder_against_existing_corpus() {
        let tmp_docs = tempdir().unwrap();
        std::fs::write(tmp_docs.path().join("a.txt"), "hello world").unwrap();
        let tmp_corpus = tempdir().unwrap();

        let e1 = MockEmbedder::new(16); // model_id = fastrag/mock-embedder-16d-v1
        index_path(
            tmp_docs.path(),
            tmp_corpus.path(),
            &ChunkingStrategy::Basic { max_chars: 1000, overlap: 0 },
            &e1,
        )
        .unwrap();

        let e2 = MockEmbedder::new(32); // different model_id
        let err = index_path(
            tmp_docs.path(),
            tmp_corpus.path(),
            &ChunkingStrategy::Basic { max_chars: 1000, overlap: 0 },
            &e2,
        )
        .unwrap_err();

        match err {
            CorpusError::EmbedderMismatch { existing, requested } => {
                assert_eq!(existing, "fastrag/mock-embedder-16d-v1");
                assert_eq!(requested, "fastrag/mock-embedder-32d-v1");
            }
            other => panic!("expected EmbedderMismatch, got {other:?}"),
        }
    }
}
```

Check `ChunkingStrategy` variant name by grep before writing — adjust if the real variant differs (e.g. `ChunkingStrategy::basic(...)`). Verify with:
```
grep -n "ChunkingStrategy::" crates/fastrag/src/corpus/mod.rs | head
```
Fix the test constructor to match.

Check `MockEmbedder::new` signature — if it needs `dim` differently, adapt. Also confirm `tempfile` is in `fastrag`'s dev-dependencies (check `crates/fastrag/Cargo.toml`); if not, add:
```toml
[dev-dependencies]
tempfile = "3"
fastrag-embed = { path = "../fastrag-embed", features = ["test-utils"] }
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p fastrag --features retrieval index_rejects_different_embedder`
Expected: FAIL — `EmbedderMismatch` variant does not exist.

- [ ] **Step 3: Add variant**

In `crates/fastrag/src/corpus/mod.rs` inside the `CorpusError` enum:

```rust
    #[error("embedder mismatch: corpus was built with `{existing}`, caller provided `{requested}`")]
    EmbedderMismatch { existing: String, requested: String },
```

- [ ] **Step 4: Enforce the check in `index_path_with_metadata`**

In `crates/fastrag/src/corpus/mod.rs`, replace the block at lines 141-151 (the `let mut index = if manifest exists { load } else { new }` block) with:

```rust
    let mut index = if corpus_dir.join("manifest.json").exists() {
        let idx = HnswIndex::load(corpus_dir)?;
        let existing = idx.manifest().embedding_model_id.clone();
        let requested = embedder.model_id();
        if existing != requested {
            return Err(CorpusError::EmbedderMismatch { existing, requested });
        }
        idx
    } else {
        let m = CorpusManifest::new(
            embedder.model_id(),
            embedder.dim(),
            current_unix_seconds(),
            manifest_chunking_strategy_from(chunking),
        );
        HnswIndex::new(embedder.dim(), m)
    };
```

- [ ] **Step 5: Run**

Run: `cargo test -p fastrag --features retrieval index_rejects_different_embedder`
Expected: PASS.

Run: `cargo test --workspace --features retrieval`
Expected: PASS.

- [ ] **Step 6: Clippy + fmt**

Run: `cargo clippy --workspace --all-targets --features retrieval -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/fastrag/src/corpus/mod.rs crates/fastrag/Cargo.toml
git commit -m "feat(corpus): reject embedder mismatches against existing manifest

Refs #31a"
```

---

## Task 9: CLI flags — `--embedder` + backend-specific options

**Files:**
- Modify: `fastrag-cli/src/args.rs`
- Modify: `fastrag-cli/Cargo.toml`

- [ ] **Step 1: Enable http-embedders in CLI**

In `fastrag-cli/Cargo.toml` find the `fastrag-embed` dependency (or the `retrieval` feature definition). Add `http-embedders` to the feature set of `fastrag-embed` that the CLI pulls in. Confirm by grepping:

```
grep -n "fastrag-embed\|retrieval" fastrag-cli/Cargo.toml
```

Adjust so the `retrieval` feature enables `fastrag-embed/http-embedders`. Example:

```toml
[features]
retrieval = ["fastrag/embedding", "fastrag/index", "fastrag/retrieval", "fastrag-embed/http-embedders"]
```

(Match the existing structure — do not duplicate features.)

- [ ] **Step 2: Add `EmbedderKindArg` + flags**

In `fastrag-cli/src/args.rs`, add below `ChunkStrategyArg`:

```rust
#[cfg(feature = "retrieval")]
#[derive(Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum EmbedderKindArg {
    Bge,
    Openai,
    Ollama,
}
```

Add the following fields to the `Index` variant (inside the `Command` enum, gated `#[cfg(feature = "retrieval")]`):

```rust
        /// Embedder backend to use.
        #[arg(long, value_enum)]
        embedder: Option<EmbedderKindArg>,

        /// OpenAI model name.
        #[arg(long, default_value = "text-embedding-3-small")]
        openai_model: String,

        /// OpenAI API base URL.
        #[arg(long, default_value = "https://api.openai.com/v1")]
        openai_base_url: String,

        /// Ollama model name.
        #[arg(long, default_value = "nomic-embed-text")]
        ollama_model: String,

        /// Ollama server URL.
        #[arg(long, default_value = "http://localhost:11434")]
        ollama_url: String,
```

Add the same five fields to `Query` and `ServeHttp` variants. Keep `model_path` unchanged.

- [ ] **Step 3: Build**

Run: `cargo build -p fastrag-cli --features retrieval`
Expected: builds.

Run: `cargo run -p fastrag-cli --features retrieval -- index --help | head -40`
Expected: new flags show up.

- [ ] **Step 4: Commit**

```bash
git add fastrag-cli/src/args.rs fastrag-cli/Cargo.toml
git commit -m "feat(cli): add --embedder and backend-specific flags

Refs #31a"
```

---

## Task 10: `embed_loader` dispatch + manifest auto-detect

**Files:**
- Modify: `fastrag-cli/src/embed_loader.rs`

- [ ] **Step 1: Rewrite the loader**

Replace `fastrag-cli/src/embed_loader.rs` with:

```rust
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fastrag::{BgeSmallEmbedder, Embedder};
use thiserror::Error;

use crate::args::EmbedderKindArg;

#[derive(Debug, Error)]
pub enum EmbedLoaderError {
    #[error("embedding model error: {0}")]
    Embed(#[from] fastrag::EmbedderError),
    #[error("unsupported model path: {0}")]
    UnsupportedModelPath(PathBuf),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse corpus manifest: {0}")]
    Manifest(String),
    #[error("embedder mismatch: corpus built with `{existing}`, --embedder specifies `{requested}`")]
    Mismatch { existing: String, requested: String },
}

#[derive(Clone)]
pub struct EmbedderOptions {
    pub kind: Option<EmbedderKindArg>,
    pub model_path: Option<PathBuf>,
    pub openai_model: String,
    pub openai_base_url: String,
    pub ollama_model: String,
    pub ollama_url: String,
}

/// Load an embedder for a write path (index). Defaults to BGE when `kind` is `None`.
pub fn load_for_write(opts: &EmbedderOptions) -> Result<Arc<dyn Embedder>, EmbedLoaderError> {
    let kind = opts.kind.unwrap_or(EmbedderKindArg::Bge);
    build(kind, opts)
}

/// Load an embedder for a read path (query, serve-http). If `kind` is `None`, parse the
/// backend out of the corpus manifest's `embedding_model_id`. If `kind` is set and the
/// resulting `model_id()` mismatches, return `Mismatch`.
pub fn load_for_read(
    corpus_dir: &Path,
    opts: &EmbedderOptions,
) -> Result<Arc<dyn Embedder>, EmbedLoaderError> {
    let manifest_path = corpus_dir.join("manifest.json");
    let bytes = std::fs::read(&manifest_path)?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| EmbedLoaderError::Manifest(e.to_string()))?;
    let existing = value
        .get("embedding_model_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| EmbedLoaderError::Manifest("missing embedding_model_id".into()))?
        .to_string();

    let (kind, model_override) = match opts.kind {
        Some(k) => (k, None),
        None => detect_from_manifest(&existing)?,
    };

    let mut effective = opts.clone();
    if let Some(m) = model_override {
        match kind {
            EmbedderKindArg::Openai => effective.openai_model = m,
            EmbedderKindArg::Ollama => effective.ollama_model = m,
            EmbedderKindArg::Bge => {}
        }
    }

    let emb = build(kind, &effective)?;
    let requested = emb.model_id();
    if requested != existing {
        return Err(EmbedLoaderError::Mismatch {
            existing,
            requested,
        });
    }
    Ok(emb)
}

fn detect_from_manifest(
    existing: &str,
) -> Result<(EmbedderKindArg, Option<String>), EmbedLoaderError> {
    if let Some(rest) = existing.strip_prefix("openai:") {
        Ok((EmbedderKindArg::Openai, Some(rest.to_string())))
    } else if let Some(rest) = existing.strip_prefix("ollama:") {
        Ok((EmbedderKindArg::Ollama, Some(rest.to_string())))
    } else if existing.starts_with("fastrag/bge") {
        Ok((EmbedderKindArg::Bge, None))
    } else {
        Err(EmbedLoaderError::Manifest(format!(
            "unrecognized embedding_model_id `{existing}`; pass --embedder explicitly"
        )))
    }
}

fn build(
    kind: EmbedderKindArg,
    opts: &EmbedderOptions,
) -> Result<Arc<dyn Embedder>, EmbedLoaderError> {
    match kind {
        EmbedderKindArg::Bge => {
            let e = match &opts.model_path {
                Some(path) => BgeSmallEmbedder::from_local(path)?,
                None => BgeSmallEmbedder::from_hf_hub()?,
            };
            Ok(Arc::new(e))
        }
        EmbedderKindArg::Openai => {
            use fastrag_embed::http::openai::OpenAIEmbedder;
            let e = OpenAIEmbedder::new(opts.openai_model.clone())?
                .with_base_url(opts.openai_base_url.clone());
            Ok(Arc::new(e))
        }
        EmbedderKindArg::Ollama => {
            use fastrag_embed::http::ollama::OllamaEmbedder;
            // Honor --ollama-url over the env default.
            unsafe { std::env::set_var("OLLAMA_HOST", &opts.ollama_url) };
            let e = OllamaEmbedder::new(opts.ollama_model.clone())?;
            Ok(Arc::new(e))
        }
    }
}
```

NOTE on `fastrag_embed` re-export: if the CLI currently only depends on `fastrag` (which re-exports `Embedder`), add `fastrag-embed` as a direct dep of `fastrag-cli` gated behind `retrieval`:

```toml
[dependencies]
fastrag-embed = { path = "../crates/fastrag-embed", features = ["http-embedders"], optional = true }
```
and in `[features]`:
```toml
retrieval = [..., "dep:fastrag-embed"]
```

Or alternatively re-export `http` from `fastrag` if that's the project pattern — check first:
```
grep -n "pub use fastrag_embed" crates/fastrag/src/lib.rs
```
Prefer the re-export if it already exists; add `pub use fastrag_embed::http;` behind the `embedding` feature if needed. Adjust imports in `embed_loader.rs` to `fastrag::http::…`.

- [ ] **Step 2: Build**

Run: `cargo build -p fastrag-cli --features retrieval`
Expected: builds.

- [ ] **Step 3: Commit**

```bash
git add fastrag-cli/src/embed_loader.rs fastrag-cli/Cargo.toml crates/fastrag/src/lib.rs
git commit -m "feat(cli): dispatch embedder kind and auto-detect from manifest

Refs #31a"
```

---

## Task 11: Wire embed_loader into `main.rs` and `http.rs`

**Files:**
- Modify: `fastrag-cli/src/main.rs`
- Modify: `fastrag-cli/src/http.rs`

- [ ] **Step 1: Locate existing `load_embedder` call sites**

Run:
```
grep -n "load_embedder\|model_path" fastrag-cli/src/main.rs fastrag-cli/src/http.rs
```
Note each site.

- [ ] **Step 2: Replace Index handler**

In `fastrag-cli/src/main.rs`, find the `Command::Index { .. }` match arm. Destructure the new fields and build `EmbedderOptions`, then:

```rust
let opts = embed_loader::EmbedderOptions {
    kind: embedder,
    model_path,
    openai_model,
    openai_base_url,
    ollama_model,
    ollama_url,
};
let embedder = embed_loader::load_for_write(&opts)?;
```

Replace the old `load_embedder(model_path)?` call with the above. Pass `embedder.as_ref()` into `index_path_with_metadata`.

- [ ] **Step 3: Replace Query handler**

Same transform, but call `load_for_read(&corpus, &opts)` instead of `load_for_write`.

- [ ] **Step 4: Replace ServeHttp handler**

Same as Query (`load_for_read`). In `fastrag-cli/src/http.rs`, wherever the embedder is constructed, accept an `Arc<dyn Embedder>` from the caller rather than constructing it locally.

- [ ] **Step 5: Build + test**

Run: `cargo build -p fastrag-cli --features retrieval`
Expected: builds.

Run: `cargo test --workspace --features retrieval`
Expected: all existing tests PASS (no behavior change on the BGE path).

- [ ] **Step 6: Clippy + fmt**

Run: `cargo clippy --workspace --all-targets --features retrieval -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add fastrag-cli/src/main.rs fastrag-cli/src/http.rs
git commit -m "feat(cli): wire new embedder dispatch into index/query/serve-http

Refs #31a"
```

---

## Task 12: CLI-level e2e tests against wiremock

**Files:**
- Create: `fastrag-cli/tests/embedder_e2e.rs`
- Modify: `fastrag-cli/Cargo.toml` (dev-deps)

- [ ] **Step 1: Add dev-deps**

In `fastrag-cli/Cargo.toml` under `[dev-dependencies]`:

```toml
wiremock = "0.6"
tokio = { version = "1", features = ["rt", "macros"] }
assert_cmd = "2"
tempfile = "3"
serde_json = "1"
```

Skip any that already exist.

- [ ] **Step 2: Write the e2e tests**

Create `fastrag-cli/tests/embedder_e2e.rs`:

```rust
#![cfg(feature = "retrieval")]

use assert_cmd::Command;
use serde_json::json;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

async fn mount_openai(server: &MockServer, dim: usize) {
    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [ { "embedding": vec![0.1_f32; dim] } ]
        })))
        .mount(server)
        .await;
}

#[test]
fn index_and_query_with_openai_backend() {
    let rt = rt();
    let (uri, _g) = rt.block_on(async {
        let s = MockServer::start().await;
        mount_openai(&s, 1536).await;
        (s.uri(), s)
    });

    let docs = tempdir().unwrap();
    std::fs::write(docs.path().join("a.txt"), "hello world").unwrap();
    let corpus = tempdir().unwrap();

    Command::cargo_bin("fastrag")
        .unwrap()
        .env("OPENAI_API_KEY", "test")
        .args([
            "index",
            docs.path().to_str().unwrap(),
            "--corpus",
            corpus.path().to_str().unwrap(),
            "--embedder",
            "openai",
            "--openai-base-url",
            &uri,
        ])
        .assert()
        .success();

    // Auto-detect on query (no --embedder flag).
    Command::cargo_bin("fastrag")
        .unwrap()
        .env("OPENAI_API_KEY", "test")
        .args([
            "query",
            "hello",
            "--corpus",
            corpus.path().to_str().unwrap(),
            "--openai-base-url",
            &uri,
        ])
        .assert()
        .success();

    // Verify manifest records the namespaced id.
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(corpus.path().join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(
        manifest["embedding_model_id"].as_str().unwrap(),
        "openai:text-embedding-3-small"
    );
}

#[test]
fn query_with_mismatched_embedder_flag_fails() {
    let rt = rt();
    let (uri, _g) = rt.block_on(async {
        let s = MockServer::start().await;
        mount_openai(&s, 1536).await;
        (s.uri(), s)
    });

    let docs = tempdir().unwrap();
    std::fs::write(docs.path().join("a.txt"), "hello").unwrap();
    let corpus = tempdir().unwrap();

    Command::cargo_bin("fastrag")
        .unwrap()
        .env("OPENAI_API_KEY", "test")
        .args([
            "index",
            docs.path().to_str().unwrap(),
            "--corpus",
            corpus.path().to_str().unwrap(),
            "--embedder",
            "openai",
            "--openai-base-url",
            &uri,
        ])
        .assert()
        .success();

    let out = Command::cargo_bin("fastrag")
        .unwrap()
        .args([
            "query",
            "hello",
            "--corpus",
            corpus.path().to_str().unwrap(),
            "--embedder",
            "bge",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("openai:text-embedding-3-small"),
        "stderr should mention existing model_id, got: {stderr}"
    );
}
```

- [ ] **Step 3: Run**

Run: `cargo test -p fastrag-cli --features retrieval --test embedder_e2e -- --test-threads=1`
Expected: PASS.

- [ ] **Step 4: Clippy + fmt**

Run: `cargo clippy --workspace --all-targets --features retrieval -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add fastrag-cli/tests/embedder_e2e.rs fastrag-cli/Cargo.toml
git commit -m "test(cli): add wiremock-backed OpenAI index/query/mismatch e2e

Refs #31a"
```

---

## Task 13: README + docs update

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md` (if retrieval section lists supported embedders)

- [ ] **Step 1: Draft README section**

Before editing, invoke the `doc-editor` skill (mandatory per CLAUDE.md) on the draft prose:

Draft to feed into doc-editor:

```markdown
### Embedder backends

FastRAG ships three embedder backends, selectable via `--embedder`:

| Backend | Flag | Model flag | Base URL flag | Auth |
|---|---|---|---|---|
| Local BGE (default) | `--embedder bge` | `--model-path <dir>` (optional) | — | — |
| OpenAI | `--embedder openai` | `--openai-model <name>` | `--openai-base-url <url>` | `OPENAI_API_KEY` env |
| Ollama | `--embedder ollama` | `--ollama-model <name>` | `--ollama-url <url>` (or `OLLAMA_HOST`) | — |

OpenAI supports `text-embedding-3-small` (1536-d) and `text-embedding-3-large` (3072-d). Ollama probes the model's dimension on startup, so any pulled embedding model works.

Once a corpus is indexed, `query` and `serve-http` auto-detect the backend from the manifest's `embedding_model_id`, so you normally don't need to repeat `--embedder` on read paths. Passing an explicit `--embedder` that disagrees with the manifest is a hard error.

#### Testing against real APIs

Real-API smoke tests are `#[ignore]`-gated. To run them:

```bash
FASTRAG_E2E_OPENAI=1 OPENAI_API_KEY=sk-... cargo test -p fastrag-embed --features http-embedders -- --ignored
```

These never run in CI.
```

Dispatch doc-editor:
```
(Invoke superpowers:write/doc-editor skill, passing the draft above, receive cleaned prose.)
```

- [ ] **Step 2: Insert into README**

Find the existing "Retrieval" section in README.md and insert the cleaned prose beneath it. Preserve surrounding headings.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: document HTTP embedder backends and real-API smoke tests

Refs #31a"
```

---

## Task 14: Final verification + push

- [ ] **Step 1: Full test run**

Run: `cargo test --workspace --features retrieval -- --test-threads=1`
Expected: all PASS (serial because tests set env vars).

- [ ] **Step 2: Full clippy gate**

Run: `cargo clippy --workspace --all-targets --features retrieval -- -D warnings`
Expected: clean.

- [ ] **Step 3: Full fmt**

Run: `cargo fmt --check`
Expected: clean.

- [ ] **Step 4: Check HTTP-feature-off build still works**

Run: `cargo build -p fastrag-embed`
Expected: builds without `reqwest` compiled in.

- [ ] **Step 5: Push and watch CI**

```bash
git push
```

Then invoke the `ci-watcher` skill as a background Haiku Agent (per CLAUDE.md — mandatory after every push).

- [ ] **Step 6: Close issue**

Once CI is green, squash-merge or directly land the branch. Ensure the final commit or PR body contains `Closes #31a`.

---

## Self-Review Notes

- **Spec coverage:** trait change (T1), errors (T2), features (T3), http module (T4), OpenAI (T5), Ollama (T6), trait-level e2e (T7), mismatch on index (T8), CLI flags (T9), dispatch + auto-detect (T10), main wiring (T11), CLI e2e incl. auto-detect + mismatch (T12), docs (T13), verify (T14). All spec sections covered.
- **Out-of-scope guardrails:** no ONNX, no Cohere, no async, no `--reset`, no cache, no cost telemetry, no token-aware chunking. Plan does not introduce any.
- **Type consistency:** `EmbedderKindArg` used throughout CLI tasks. `EmbedderOptions` struct defined once in Task 10 and referenced in Task 11. `model_id()` returns `String` everywhere after Task 1.
- **Known caveat:** tests mutate `OPENAI_API_KEY` and `OLLAMA_HOST` env vars and must run with `--test-threads=1`; this is explicit in Task 6/7/12/14 run commands.
