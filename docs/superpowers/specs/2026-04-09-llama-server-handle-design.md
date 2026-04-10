# Spec: `LlamaServerHandle` subprocess lifecycle manager

**Parent plan:** `docs/superpowers/plans/2026-04-09-llama-cpp-backend.md` — Task 3
**Date:** 2026-04-09
**Crate:** `fastrag-embed` (feature `llama-cpp`)

## Goal

Provide a RAII handle that spawns a `llama-server` subprocess, waits until its
`/health` endpoint reports ready, and terminates the subprocess deterministically
on drop. This is the foundation the HTTP embedding client (Task 4) and the
Qwen3 preset (Task 5) build on.

## Non-goals

- Model path resolution (Task 6).
- Version enforcement (Task 7).
- CLI wiring / `fastrag doctor` (Task 8).
- The HTTP embedding request/response (Task 4).

The config struct stays minimal for T3; later tasks grow it as they need.

## Public API

```rust
// src/llama_cpp/handle.rs

pub struct LlamaServerConfig {
    pub binary_path: PathBuf,
    pub port: u16,
    pub health_timeout: Duration, // default 30s
    pub extra_args: Vec<String>,
}

pub struct LlamaServerHandle {
    child: std::process::Child,
    base_url: String,
    http: reqwest::blocking::Client,
}

impl LlamaServerHandle {
    pub fn spawn(cfg: LlamaServerConfig) -> Result<Self, EmbedError>;
    pub fn base_url(&self) -> &str;
    pub fn client(&self) -> &reqwest::blocking::Client;
    pub fn pid(&self) -> u32;
}

impl Drop for LlamaServerHandle { /* graceful shutdown */ }
```

Re-exported from `src/llama_cpp/mod.rs` as `pub use handle::{LlamaServerHandle, LlamaServerConfig};`.

## Behaviour

### `spawn`

1. `std::process::Command::new(cfg.binary_path).arg("--port").arg(port).args(extra_args).stdout(Stdio::null()).stderr(Stdio::piped()).spawn()`.
2. Build a blocking `reqwest::Client` with a 500ms per-request timeout.
3. Poll loop, every 100ms:
   - If `child.try_wait()?` is `Some(status)` → return `EmbedError::LlamaServerExitedEarly { status }`.
   - `GET http://127.0.0.1:{port}/health` — on 200, break.
   - If total elapsed ≥ `cfg.health_timeout` → kill child, return `EmbedError::LlamaServerHealthTimeout { port, waited }`.
4. Return the handle.

Errors (new `EmbedError` variants):
- `LlamaServerSpawn(std::io::Error)` — `Command::spawn` failed.
- `LlamaServerExitedEarly { status: std::process::ExitStatus }`.
- `LlamaServerHealthTimeout { port: u16, waited: Duration }`.

### `Drop`

- `#[cfg(unix)]`: send `SIGTERM` via `nix::sys::signal::kill(Pid::from_raw(pid as i32), SIGTERM)`. Loop on `child.try_wait()` every 50ms for up to 2s. If still alive, `child.kill()`.
- `#[cfg(not(unix))]`: `child.kill()` directly.
- All errors swallowed in `Drop`; best-effort cleanup.

### Dependencies

Add to `crates/fastrag-embed/Cargo.toml`, inside the `llama-cpp` feature gate (unix-only for `nix`):

```toml
[target.'cfg(unix)'.dependencies]
nix = { version = "0.29", features = ["signal"], optional = true }

[features]
llama-cpp = ["http-embedders", "dep:nix"]
```

## Test strategy

### Fake binary

`crates/fastrag-embed/tests/support/fake_llama_server.rs`, registered as:

```toml
[[bin]]
name = "fake-llama-server"
path = "tests/support/fake_llama_server.rs"
required-features = ["llama-cpp"]
```

Behaviour:
- Parse `--port <u16>`. Also supports `--never-ready` (for timeout test).
- Bind `TcpListener::bind(("127.0.0.1", port))`. If binding fails, exit non-zero.
- Accept loop reading request line by line:
  - `/health` → `200 OK`, body `OK` (or, if `--never-ready`, sleep-and-drop).
  - `/embedding` → `200 OK`, body `{"embedding":[[0.0]]}` (placeholder for T4).
  - else → `404`.
- Terminates on SIGTERM via default signal handler (no ctrlc crate needed).

### Integration tests

`crates/fastrag-embed/tests/llama_server_handle.rs`, gated `#![cfg(feature = "llama-cpp")]`.

**Test 1 — `spawns_polls_health_and_drops_cleanly`**
1. Bind `127.0.0.1:0`, record port, drop listener.
2. `LlamaServerHandle::spawn(cfg)` with `binary_path = env!("CARGO_BIN_EXE_fake-llama-server")`.
3. Assert `h.client().get(format!("{}/health", h.base_url())).send()?.status() == 200`, body bytes `== b"OK"`.
4. Record `pid = h.pid()`, then `drop(h)`.
5. Poll `GET /health` up to 3s with 200ms timeouts; assert that at least one request fails with a transport error (connection refused). Concrete check that the process is gone.

**Test 2 — `spawn_errors_when_never_ready`**
1. Free port as above.
2. Spawn with `extra_args = vec!["--never-ready".into()]`, `health_timeout = Duration::from_millis(500)`.
3. Assert `Err(EmbedError::LlamaServerHealthTimeout { port: p, .. })` where `p` equals the chosen port.

Both tests assert concrete values (status codes, byte content, pid, error variant + field). No rubber-stamps.

## TDD order

1. Add `[[bin]]` entry + write `fake_llama_server.rs`.
2. Write Test 1. Run → red (compile error: no `LlamaServerHandle`).
3. Implement `handle.rs` minimally. Run → green.
4. Write Test 2. Run → red (no `--never-ready` in fake, or wrong error variant).
5. Add `--never-ready` to fake + proper timeout error path. Run → green.
6. `cargo clippy -p fastrag-embed --features llama-cpp --all-targets -- -D warnings`.
7. Commit: `feat(embed): llama-server subprocess lifecycle manager`.

## Definition of done

- `cargo test -p fastrag-embed --features llama-cpp` green (both tests pass).
- `cargo test -p fastrag-embed` still green (feature off; new code gated).
- Clippy clean under `--features llama-cpp --all-targets -- -D warnings`.
- No regressions to `legacy-candle` or `http-embedders` feature combinations.
