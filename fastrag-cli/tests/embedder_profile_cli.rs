#![cfg(feature = "retrieval")]

use clap::Parser;
use fastrag_cli::args::Cli;

#[test]
fn query_accepts_embedder_profile_and_config() {
    let cli = Cli::try_parse_from([
        "fastrag",
        "query",
        "search term",
        "--corpus",
        "./corpus",
        "--top-k",
        "5",
        "--config",
        "./fastrag.toml",
        "--embedder-profile",
        "vams",
    ])
    .expect("CLI should parse");

    let debug = format!("{cli:?}");
    assert!(
        debug.contains("vams"),
        "debug output did not contain profile: {debug}"
    );
    assert!(
        debug.contains("fastrag.toml"),
        "debug output did not contain config path: {debug}"
    );
}

#[test]
fn old_embedder_flag_is_rejected() {
    let err = Cli::try_parse_from([
        "fastrag",
        "query",
        "search term",
        "--corpus",
        "./corpus",
        "--top-k",
        "5",
        "--embedder",
        "ollama",
    ])
    .expect_err("old embedder flag should be rejected");

    let message = err.to_string();
    assert!(
        message.contains("--embedder"),
        "parse error should mention the rejected flag, got: {message}"
    );
}
