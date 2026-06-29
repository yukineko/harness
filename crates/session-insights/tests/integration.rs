//! End-to-end tests for the session-insights binary.
//!
//! `record` (PostToolUse) and `stop` (Stop) are hooks: they only ever write to
//! their own state, never block a turn, and always exit 0. `report` and
//! `status` are read-only CLI subcommands.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_home() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir =
        std::env::temp_dir().join(format!("session-insights-it-{}-{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Run `session-insights <args>` with `payload` on stdin. HOME is isolated so
/// the state store (~/.session-insights) is never the real one. An optional
/// shared `home` keeps record→report in the same store.
fn run_in(home: &PathBuf, args: &[&str], payload: &str) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_session-insights");
    let mut child = Command::new(bin)
        .args(args)
        .current_dir(home)
        .env("HOME", home)
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

fn run(args: &[&str], payload: &str) -> (i32, String) {
    run_in(&temp_home(), args, payload)
}

#[test]
fn help_describes_session_metrics() {
    let (code, stdout) = run(&["--help"], "");
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Per-session work metrics"),
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
fn report_on_empty_store_reports_no_sessions() {
    let (code, stdout) = run(&["report"], "");
    assert_eq!(code, 0);
    assert!(
        stdout.contains("no matching session"),
        "expected empty-store hint, got: {stdout}"
    );
}

#[test]
fn record_then_report_round_trips() {
    // Record a tool call, then report on the same session in the same HOME.
    let home = temp_home();
    let payload = r#"{"hook_event_name":"PostToolUse","session_id":"it-roundtrip","tool_name":"Read","tool_input":{"file_path":"a.rs"}}"#;
    let (rc, _) = run_in(&home, &["record"], payload);
    assert_eq!(rc, 0, "record hook must exit 0");
    let (code, stdout) = run_in(&home, &["report", "--session", "it-roundtrip"], "");
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Read"),
        "expected the recorded tool in the report, got: {stdout}"
    );
}

#[test]
fn record_hook_survives_garbage_stdin() {
    let (code, _stdout) = run(&["record"], "not json");
    assert_eq!(
        code, 0,
        "fail-soft: garbage stdin must never break the turn"
    );
}

#[test]
fn stop_hook_survives_empty_stdin() {
    let (code, _stdout) = run(&["stop"], "");
    assert_eq!(code, 0, "fail-soft: empty stdin must never break the turn");
}
