//! End-to-end tests for the stuckguard binary.
//!
//! `watch` is the PostToolUse hook: it can only ADVISE (inject context); it can
//! never block a tool call or end a turn, so it must ALWAYS exit 0. `status` is
//! a read-only CLI subcommand.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_home() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("stuckguard-it-{}-{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Run `stuckguard <args>` with `payload` on stdin in an isolated HOME/CWD.
fn run(args: &[&str], payload: &str) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_stuckguard");
    let home = temp_home();
    let mut child = Command::new(bin)
        .args(args)
        .current_dir(&home)
        .env("HOME", &home)
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

#[test]
fn help_describes_the_detector() {
    let (code, stdout) = run(&["--help"], "");
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Stuck-loop detector"),
        "expected about string, got: {stdout}"
    );
}

#[test]
fn status_reports_resolved_config() {
    let (code, stdout) = run(&["status"], "");
    assert_eq!(code, 0);
    assert!(
        stdout.contains("enabled:"),
        "expected status output, got: {stdout}"
    );
}

#[test]
fn watch_hook_valid_payload_exits_zero() {
    // A single, non-repeating PostToolUse event: no stuck pattern → silent.
    let payload = r#"{"hook_event_name":"PostToolUse","session_id":"it","tool_name":"Read","tool_input":{"file_path":"a.rs"}}"#;
    let (code, stdout) = run(&["watch"], payload);
    assert_eq!(code, 0, "PostToolUse hook must always exit 0");
    assert!(
        stdout.trim().is_empty(),
        "a single event trips no nudge, got: {stdout}"
    );
}

#[test]
fn watch_hook_survives_garbage_stdin() {
    let (code, _stdout) = run(&["watch"], "not json");
    assert_eq!(
        code, 0,
        "fail-soft: garbage stdin must never break the turn"
    );
}

#[test]
fn watch_hook_survives_empty_stdin() {
    let (code, _stdout) = run(&["watch"], "");
    assert_eq!(code, 0, "fail-soft: empty stdin must never break the turn");
}
