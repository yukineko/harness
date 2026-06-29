//! End-to-end tests for the real built `taskprog` binary.
//!
//! taskprog is a clap CLI with two lifecycle-hook subcommands (`SessionStart`,
//! `Stop`) that run under `harness_core::hook::run_hook` and therefore must NEVER
//! break a turn (always exit 0, even on garbage stdin), plus plain read-only
//! subcommands (`show`, `status`, `--help`).

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

/// Run the binary with `args` and `payload` on stdin, inside an isolated cwd +
/// HOME so nothing touches the real filesystem state. Returns (exit_code, stdout).
fn run(args: &[&str], payload: &str) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_taskprog");
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
    if let Some(mut child_stdin) = child.stdin.take() {
        let _ = child_stdin.write_all(payload.as_bytes());
    }
    let out = child.wait_with_output().expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn unique_dir() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let id = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("taskprog-it-{}-{}", std::process::id(), id))
}

#[test]
fn help_lists_the_subcommands() {
    let (code, stdout) = run(&["--help"], "");
    assert_eq!(code, 0, "--help must exit 0");
    assert!(
        stdout.contains("Progress-file plugin for Claude Code"),
        "expected the about string, got: {stdout}"
    );
}

#[test]
fn session_start_valid_payload_exits_zero() {
    // A minimal valid SessionStart HookInput. With no progress file present in the
    // isolated cwd, the hook injects nothing but must still exit 0.
    let payload = r#"{"hook_event_name":"SessionStart"}"#;
    let (code, _stdout) = run(&["session-start"], payload);
    assert_eq!(code, 0, "SessionStart hook must always exit 0");
}

#[test]
fn session_start_malformed_stdin_exits_zero() {
    // The load-bearing invariant: garbage stdin must never break a turn.
    let (code, _stdout) = run(&["session-start"], "not json at all");
    assert_eq!(code, 0, "malformed stdin must still exit 0 (fail-soft)");
}

#[test]
fn stop_malformed_stdin_exits_zero() {
    let (code, _stdout) = run(&["stop"], "");
    assert_eq!(code, 0, "Stop hook must exit 0 on empty stdin (fail-soft)");
}

#[test]
fn status_runs_read_only_without_panic() {
    // `status` prints the resolved config for the (empty, isolated) cwd.
    let (code, stdout) = run(&["status"], "");
    assert_eq!(code, 0, "status is a benign read-only command");
    assert!(
        stdout.contains("enabled:") && stdout.contains("progress_file:"),
        "expected status fields, got: {stdout}"
    );
}
