//! Integration tests for `LlamaServerHandle` — Task 3 of the llama.cpp
//! backend plan. Uses the in-crate `fake-llama-server` binary (see
//! `tests/support/fake_llama_server.rs`).

#![cfg(feature = "llama-cpp")]

use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use fastrag_embed::EmbedError;
use fastrag_embed::llama_cpp::{LlamaServerConfig, LlamaServerHandle};

fn free_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    let port = l.local_addr().expect("local_addr").port();
    drop(l);
    port
}

fn fake_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake-llama-server"))
}

#[test]
fn spawns_polls_health_and_drops_cleanly() {
    let port = free_port();
    let cfg = LlamaServerConfig {
        binary_path: fake_binary(),
        port,
        health_timeout: Duration::from_secs(5),
        extra_args: vec![],
        skip_version_check: true,
    };

    let handle = LlamaServerHandle::spawn(cfg).expect("spawn must succeed");
    assert_eq!(handle.base_url(), format!("http://127.0.0.1:{port}"));

    let resp = handle
        .client()
        .get(format!("{}/health", handle.base_url()))
        .send()
        .expect("health GET");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(resp.bytes().expect("body").as_ref(), b"OK");

    let pid_before = handle.pid();
    assert!(pid_before > 0);

    drop(handle);

    // After drop the process should be gone and the port unreachable.
    let probe = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(200))
        .build()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut last_err: Option<String> = None;
    let mut died = false;
    while Instant::now() < deadline {
        match probe.get(format!("http://127.0.0.1:{port}/health")).send() {
            Ok(_) => thread::sleep(Duration::from_millis(100)),
            Err(e) => {
                last_err = Some(e.to_string());
                died = true;
                break;
            }
        }
    }
    assert!(
        died,
        "expected /health to become unreachable after drop; last_err={last_err:?}"
    );
}

#[test]
fn spawn_errors_when_never_ready() {
    let port = free_port();
    let cfg = LlamaServerConfig {
        binary_path: fake_binary(),
        port,
        health_timeout: Duration::from_millis(500),
        extra_args: vec!["--never-ready".into()],
        skip_version_check: true,
    };

    let err = match LlamaServerHandle::spawn(cfg) {
        Err(e) => e,
        Ok(_) => panic!("spawn must time out"),
    };
    match err {
        EmbedError::LlamaServerHealthTimeout { port: p, .. } => {
            assert_eq!(p, port);
        }
        other => panic!("expected LlamaServerHealthTimeout, got {other:?}"),
    }
}

#[test]
fn spawn_fails_when_version_too_old() {
    let port = free_port();
    let cfg = LlamaServerConfig {
        binary_path: fake_binary(),
        port,
        health_timeout: Duration::from_secs(5),
        extra_args: vec!["--fake-version".into(), "1000".into()],
        skip_version_check: false,
    };

    let err = match LlamaServerHandle::spawn(cfg) {
        Err(e) => e,
        Ok(_) => panic!("spawn must fail for old version"),
    };
    match err {
        EmbedError::LlamaServerVersionTooOld { found, minimum } => {
            assert_eq!(found, 1000);
            assert_eq!(minimum, fastrag_embed::llama_cpp::MIN_LLAMA_SERVER_BUILD);
        }
        other => panic!("expected LlamaServerVersionTooOld, got {other:?}"),
    }
}

#[test]
fn spawn_succeeds_when_version_ok() {
    let port = free_port();
    let cfg = LlamaServerConfig {
        binary_path: fake_binary(),
        port,
        health_timeout: Duration::from_secs(5),
        extra_args: vec!["--fake-version".into(), "6000".into()],
        skip_version_check: false,
    };

    let handle = LlamaServerHandle::spawn(cfg).expect("spawn must succeed with good version");
    assert_eq!(handle.base_url(), format!("http://127.0.0.1:{port}"));
    drop(handle);
}
