//! CLI integration tests for `fastrag doctor`.

use std::process::Command;

#[test]
fn doctor_exits_zero_and_prints_header() {
    let binary = env!("CARGO_BIN_EXE_fastrag");
    let output = Command::new(binary).arg("doctor").output().unwrap();
    assert!(output.status.success(), "doctor exited non-zero");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("fastrag doctor"),
        "missing header, got: {stdout}"
    );
    assert!(
        stdout.contains("llama-server"),
        "missing llama-server check, got: {stdout}"
    );
}

#[test]
fn doctor_shows_not_found_when_path_is_empty() {
    let binary = env!("CARGO_BIN_EXE_fastrag");
    let output = Command::new(binary)
        .env("PATH", "")
        .env_remove("LLAMA_SERVER_PATH")
        .arg("doctor")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("NOT FOUND"),
        "expected NOT FOUND, got: {stdout}"
    );
}
