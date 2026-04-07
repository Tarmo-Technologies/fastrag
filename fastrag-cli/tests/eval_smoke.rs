use std::fs;
use std::process::Command;

#[test]
fn cli_eval_smoke() {
    let binary = env!("CARGO_BIN_EXE_fastrag");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../crates/fastrag-eval/tests/fixtures/tiny.json");
    let report = std::env::temp_dir().join("fastrag-eval-smoke.json");

    let output = Command::new(binary)
        .args([
            "eval",
            "--dataset",
            fixture.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ])
        .output()
        .expect("command should run");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(report.exists());
    let json = fs::read_to_string(&report).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["schema_version"], 2);
    assert_eq!(parsed["dataset"], "tiny-synthetic");
    assert!(parsed["top_k"].is_number());
    assert!(parsed["git_rev"].is_string());
    assert!(parsed["fastrag_version"].is_string());
}
