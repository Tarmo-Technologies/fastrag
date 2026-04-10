//! Minimal fake `llama-server` used by integration tests for
//! `LlamaServerHandle`. Hand-rolled HTTP over `TcpListener` — no async, no
//! external deps beyond std.
//!
//! Supported flags:
//!   --port <u16>       bind port (required)
//!   --never-ready      accept connections but never return 200 on /health
//!   --version          print version string and exit
//!   --fake-version <N> set the build number printed by --version (default: 9999)

use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let mut port: Option<u16> = None;
    let mut never_ready = false;
    let mut print_version = false;
    let mut fake_build: u32 = 9999;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                port = args.next().and_then(|s| s.parse::<u16>().ok());
            }
            "--never-ready" => never_ready = true,
            "--version" => print_version = true,
            "--fake-version" => {
                fake_build = args
                    .next()
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(9999);
            }
            _ => { /* ignore unknown flags — real llama-server has many */ }
        }
    }

    if print_version {
        // Mimic real llama-server output: "version: b<N> (abc1234)"
        println!("version: b{fake_build} (deadbeef)");
        return ExitCode::SUCCESS;
    }

    let Some(port) = port else {
        eprintln!("fake-llama-server: --port <u16> is required");
        return ExitCode::from(2);
    };

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("fake-llama-server: bind failed on port {port}: {e}");
            return ExitCode::from(3);
        }
    };

    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let mut reader = BufReader::new(stream.try_clone().expect("clone tcp stream"));
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            continue;
        }
        // Drain headers, capture Content-Length.
        let mut content_length: usize = 0;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() || line == "\r\n" || line.is_empty() {
                break;
            }
            if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                content_length = v.trim().parse().unwrap_or(0);
            }
        }
        // Read body (may be empty for GET).
        let mut body_buf = vec![0u8; content_length];
        if content_length > 0 {
            let _ = reader.read_exact(&mut body_buf);
        }
        let body_str = String::from_utf8_lossy(&body_buf);
        // Count inputs by tallying quoted strings after `"input":[`. This is
        // hacky but good enough for a fake — real parsing would pull in a
        // json crate.
        let input_count = count_inputs(&body_str);

        let path = request_line.split_whitespace().nth(1).unwrap_or("");
        let response: Vec<u8> = if path.starts_with("/v1/embeddings") {
            let n = input_count.max(1);
            let mut items = Vec::with_capacity(n);
            for i in 0..n {
                // Dim 1024 (Qwen3-Embedding-0.6B). Fill with a per-index
                // constant so tests can tell vectors apart.
                let v = (i as f32 + 1.0) * 0.1;
                let vec_str = format!("[{}]", vec![format!("{v}"); 1024].join(","));
                items.push(format!(r#"{{"embedding":{vec_str},"index":{i}}}"#));
            }
            let body = format!(r#"{{"data":[{}]}}"#, items.join(","));
            let mut r = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .into_bytes();
            r.extend_from_slice(body.as_bytes());
            r
        } else if path.starts_with("/health") {
            if never_ready {
                b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 11\r\nConnection: close\r\n\r\nnot ready\r\n".to_vec()
            } else {
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK".to_vec()
            }
        } else if path.starts_with("/embedding") {
            let body = br#"{"embedding":[[0.0]]}"#;
            let mut r = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .into_bytes();
            r.extend_from_slice(body);
            r
        } else {
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
        };

        let _ = stream.write_all(&response);
        let _ = stream.flush();
    }

    ExitCode::SUCCESS
}

/// Count top-level string entries in the first `"input":[...]` array of a
/// JSON body. Good enough for test fixtures; fails on nested arrays/escapes.
fn count_inputs(body: &str) -> usize {
    let Some(start) = body.find("\"input\"") else {
        return 0;
    };
    let Some(rel_bracket) = body[start..].find('[') else {
        return 0;
    };
    let open = start + rel_bracket + 1;
    let Some(rel_close) = body[open..].find(']') else {
        return 0;
    };
    let slice = &body[open..open + rel_close];
    slice.matches('"').count() / 2
}
