//! End-to-end test of the Qwen3Embed600mQ8 preset against the in-crate
//! fake llama-server binary. Verifies that load() spawns a subprocess,
//! embed_query / embed_passage round-trip through /v1/embeddings with the
//! correct shape, and dropping the preset terminates the subprocess.

#![cfg(feature = "llama-cpp")]

use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use fastrag_embed::llama_cpp::{LlamaServerConfig, Qwen3Embed600mQ8};
use fastrag_embed::{Embedder, PassageText, QueryText};

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
fn preset_round_trip_and_shutdown() {
    let port = free_port();
    let cfg = LlamaServerConfig {
        binary_path: fake_binary(),
        port,
        health_timeout: Duration::from_secs(5),
        extra_args: vec![],
        skip_version_check: true,
    };

    let preset = Qwen3Embed600mQ8::load(cfg).expect("preset must load");
    assert_eq!(preset.base_url(), format!("http://127.0.0.1:{port}"));

    // Batch query — assert shape and distinct per-index values from the fake.
    let q = preset
        .embed_query(&[
            QueryText::new("a"),
            QueryText::new("b"),
            QueryText::new("c"),
        ])
        .expect("embed_query");
    assert_eq!(q.len(), 3);
    assert_eq!(q[0].len(), Qwen3Embed600mQ8::DIM);
    assert_eq!(q[1].len(), Qwen3Embed600mQ8::DIM);
    assert_eq!(q[2].len(), Qwen3Embed600mQ8::DIM);
    // Fake returns (i+1)*0.1, so vectors are distinguishable.
    assert!((q[0][0] - 0.1).abs() < 1e-5, "got {}", q[0][0]);
    assert!((q[1][0] - 0.2).abs() < 1e-5, "got {}", q[1][0]);
    assert!((q[2][0] - 0.3).abs() < 1e-5, "got {}", q[2][0]);

    // Passage path uses the same wire protocol.
    let p = preset
        .embed_passage(&[PassageText::new("hello"), PassageText::new("world")])
        .expect("embed_passage");
    assert_eq!(p.len(), 2);
    assert_eq!(p[0].len(), Qwen3Embed600mQ8::DIM);
    assert!((p[0][0] - 0.1).abs() < 1e-5);
    assert!((p[1][0] - 0.2).abs() < 1e-5);

    drop(preset);

    // Subprocess should be gone — /health becomes unreachable.
    let probe = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(200))
        .build()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut died = false;
    while Instant::now() < deadline {
        if probe
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .is_err()
        {
            died = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(died, "subprocess still alive after preset drop");
}
