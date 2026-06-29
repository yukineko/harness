//! End-to-end tests for the real built `tdd` binary.
//!
//! tdd is a clap CLI. `gate` is the Stop hook — it must NEVER break a turn:
//! garbage stdin (a *harness* error) exits 0 and allows the stop. `verify` is a
//! deterministic read-only gate that exits 1 when no proofs exist.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

/// Run the binary with `args` and `payload` on stdin, inside an isolated cwd +
/// HOME. Returns (exit_code, stdout).
fn run(args: &[&str], payload: &str) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_tdd");
    let dir = unique_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let mut child = Command::new(bin)
        .args(args)
        .current_dir(&dir)
        .env("HOME", &dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary spawns");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn unique_dir() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let id = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("tdd-it-{}-{}", std::process::id(), id))
}

#[test]
fn help_describes_the_gate() {
    let (code, stdout) = run(&["--help"], "");
    assert_eq!(code, 0, "--help must exit 0");
    assert!(
        stdout.contains("Test-first gate for Claude Code"),
        "expected the about string, got: {stdout}"
    );
}

#[test]
fn gate_malformed_stdin_allows_the_stop() {
    // The load-bearing invariant: a harness-level error (garbage stdin) must
    // never trap a turn. `gate` treats unparseable input as interactive and the
    // empty isolated cwd has no config, so it allows the stop with exit 0.
    let (code, _stdout) = run(&["gate"], "not json");
    assert_eq!(code, 0, "gate must allow the stop on malformed stdin");
}

#[test]
fn gate_valid_hook_payload_exits_zero() {
    // A valid Stop HookInput pointed at an empty (non-git) isolated cwd: nothing
    // changed, so there is nothing to block — exit 0.
    let payload = r#"{"hook_event_name":"Stop","cwd":"."}"#;
    let (code, _stdout) = run(&["gate"], payload);
    assert_eq!(code, 0, "gate must exit 0 toward Claude");
}

#[test]
fn verify_without_proofs_exits_one() {
    // Deterministic read-only gate: with no RED/GREEN proof artifacts present,
    // `verify` reports the missing proof and exits 1.
    let (code, _stdout) = run(&["verify", "--task", "demo"], "");
    assert_eq!(code, 1, "verify exits 1 when proofs are absent");
}
