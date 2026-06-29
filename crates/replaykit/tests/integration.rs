//! End-to-end tests: run the real built `replaykit` binary. It is a plain CLI
//! gate (NOT a lifecycle hook), so it follows the 0/1/2 exit policy: 0 = replay
//! matched, 1 = regression, 2 = harness error (missing/malformed input). Tests
//! run against an isolated temp dir so the real `~/.tracekit` is never touched.

use std::path::PathBuf;
use std::process::Command;

/// A unique throwaway directory for one test, used as both HOME and a fixture dir.
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "replaykit-{tag}-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run the binary with `args` in an isolated HOME. Returns (exit_code, stdout).
fn run(args: &[&str], home: &PathBuf) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_replaykit");
    let out = Command::new(bin)
        .args(args)
        .env("HOME", home)
        .output()
        .expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn help_describes_replay_harness() {
    let home = temp_dir("help");
    let (code, stdout) = run(&["--help"], &home);
    std::fs::remove_dir_all(&home).ok();
    assert_eq!(code, 0, "--help must exit 0");
    assert!(
        stdout.contains("replay") && stdout.contains("golden"),
        "expected the about string, got: {stdout}"
    );
}

#[test]
fn version_prints_binary_name() {
    let home = temp_dir("version");
    let (code, stdout) = run(&["--version"], &home);
    std::fs::remove_dir_all(&home).ok();
    assert_eq!(code, 0, "--version must exit 0");
    assert!(
        stdout.contains("replaykit"),
        "expected the binary name, got: {stdout}"
    );
}

#[test]
fn extract_missing_spans_is_harness_error() {
    // No spans file under the isolated HOME → documented exit code 2.
    let home = temp_dir("extract-missing");
    let (code, _stdout) = run(&["extract", "--run", "does-not-exist"], &home);
    std::fs::remove_dir_all(&home).ok();
    assert_eq!(
        code, 2,
        "a missing spans file must surface as harness-error exit 2"
    );
}

#[test]
fn verify_healthy_fixture_passes() {
    // A self-consistent summary fixture: recomputed aggregates match its pins,
    // so verify reports a clean replay (exit 0).
    let dir = temp_dir("verify-ok");
    let fixture = dir.join("healthy.json");
    let json = r#"{
  "run_id": "test-run",
  "steps": [
    {"span_id": "a", "name": "n", "phase": "interpreter", "status": "ok", "ms": 1, "cost_usd": 0.1},
    {"span_id": "b", "name": "n", "phase": "worker", "status": "verified", "ms": 1, "cost_usd": 0.2},
    {"span_id": "c", "name": "n", "phase": "verifier", "status": "ok", "ms": 1}
  ],
  "expect": {
    "phases": ["interpreter", "worker", "verifier"],
    "max_error_count": 0,
    "max_cost_usd": 0.3
  }
}"#;
    std::fs::write(&fixture, json).unwrap();
    let (code, stdout) = run(&["verify", fixture.to_str().unwrap()], &dir);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(code, 0, "a self-consistent fixture must pass (exit 0)");
    assert!(
        stdout.contains("replay matched (pass)") && stdout.contains("test-run"),
        "expected the pass line, got: {stdout}"
    );
}
