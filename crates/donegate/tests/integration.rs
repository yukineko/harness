//! End-to-end tests for the real built `donegate` binary. donegate is a clap
//! subcommand CLI; its `gate` subcommand is the Stop hook (reads a JSON
//! HookInput from stdin and must ALWAYS exit 0 toward Claude — it blocks via a
//! `decision` field, never via a non-zero exit, so it can't trap a turn).

use std::io::Write;
use std::process::{Command, Stdio};

/// A fresh isolated HOME + working dir so the gate never touches the real
/// `~/.donegate` and finds no project `donegate.toml` (→ no checks → allow stop).
fn isolated_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("donegate-it-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run a non-hook subcommand. Returns (exit_code, stdout).
fn run(args: &[&str]) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_donegate");
    let dir = isolated_dir("cli");
    let out = Command::new(bin)
        .args(args)
        .current_dir(&dir)
        .env("HOME", &dir)
        .output()
        .expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

/// Run `gate` feeding `payload` on stdin (Stop-hook mode). Returns (code, stdout).
fn run_gate(payload: &str) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_donegate");
    let dir = isolated_dir("gate");
    let mut child = Command::new(bin)
        .arg("gate")
        .current_dir(&dir)
        .env("HOME", &dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary spawns");
    if let Some(mut child_stdin) = child.stdin.take() {
        let _ = child_stdin.write_all(payload.as_bytes());
    }
    let out = child.wait_with_output().expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn help_exits_zero_and_describes_the_gate() {
    let (code, stdout) = run(&["--help"]);
    assert_eq!(code, 0, "--help must exit 0");
    assert!(
        stdout.contains("Completion-verification gate"),
        "expected the about string, got: {stdout}"
    );
}

#[test]
fn status_prints_resolved_config() {
    // `status` is read-only: prints the resolved config for the cwd.
    let (code, stdout) = run(&["status"]);
    assert_eq!(code, 0, "status must exit 0");
    assert!(stdout.contains("enabled:"), "got: {stdout}");
    assert!(stdout.contains("checks:"), "got: {stdout}");
}

#[test]
fn gate_with_valid_stop_payload_and_no_checks_allows_stop() {
    // Valid Stop hook input, but the isolated dir has no donegate.toml → no
    // checks → the gate allows the stop and exits 0 (the never-trap invariant).
    let payload = r#"{"hook_event_name":"Stop","session_id":"s1","cwd":"."}"#;
    let (code, _stdout) = run_gate(payload);
    assert_eq!(code, 0, "gate must always exit 0 toward Claude");
}

#[test]
fn gate_with_malformed_stdin_still_exits_zero() {
    // Fail-soft: malformed stdin must never break the turn.
    let (code, _) = run_gate("not json");
    assert_eq!(code, 0, "malformed stdin must still exit 0");
}

#[test]
fn gate_with_empty_stdin_still_exits_zero() {
    let (code, _) = run_gate("");
    assert_eq!(code, 0, "empty stdin must still exit 0");
}
