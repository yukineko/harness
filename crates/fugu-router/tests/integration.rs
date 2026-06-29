//! End-to-end tests for the `fugu-router` binary.
//!
//! `Prompt` is a UserPromptSubmit hook (runs under `run_hook` → must ALWAYS exit
//! 0, never breaking the turn). The other subcommands are an ordinary CLI. Tests
//! that touch the store run with an isolated `HOME` so the real
//! `~/.fugu-router/episodes.jsonl` is never read or written (the config resolves
//! the store path via `dirs::home_dir()`, which honors `$HOME` on Linux).

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// A unique, isolated temp HOME for one test (so store paths never hit real $HOME).
fn temp_home(tag: &str) -> PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("fugu-router-test-{tag}-{pid}-{nanos}"));
    std::fs::create_dir_all(&dir).expect("create temp home");
    dir
}

/// Run the binary with `args` and `payload` on stdin, under an isolated HOME.
/// Returns (exit_code, stdout).
fn run_in(home: &PathBuf, args: &[&str], payload: &str) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_fugu-router");
    let mut child = Command::new(bin)
        .args(args)
        .env("HOME", home)
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
fn help_lists_route_subcommand() {
    let home = temp_home("help");
    let (code, stdout) = run_in(&home, &["--help"], "");
    assert_eq!(code, 0, "--help must exit 0");
    assert!(
        stdout.contains("route"),
        "expected the route subcommand in --help, got: {stdout}"
    );
}

#[test]
fn prompt_hook_exits_zero_on_valid_payload() {
    // A real UserPromptSubmit payload. With an empty store the hook emits no
    // additionalContext, but the load-bearing invariant is exit 0.
    let home = temp_home("prompt-valid");
    let payload =
        r#"{"hook_event_name":"UserPromptSubmit","prompt":"add a login feature to the api"}"#;
    let (code, _stdout) = run_in(&home, &["prompt"], payload);
    assert_eq!(code, 0, "Prompt hook must always exit 0");
}

#[test]
fn prompt_hook_exits_zero_on_malformed_stdin() {
    // Fail-soft invariant: garbage stdin must never break the turn.
    let home = temp_home("prompt-bad");
    let (code, stdout) = run_in(&home, &["prompt"], "not json");
    assert_eq!(code, 0, "Prompt hook must exit 0 even on malformed stdin");
    assert!(
        stdout.trim().is_empty(),
        "no actionable prompt → no output, got: {stdout}"
    );
}

#[test]
fn stats_on_empty_store_reports_zero_episodes() {
    // Read-only subcommand against a fresh (nonexistent) store in temp HOME.
    let home = temp_home("stats");
    let (code, stdout) = run_in(&home, &["stats"], "");
    assert_eq!(code, 0, "stats must exit 0 on an empty store");
    assert!(
        stdout.contains("episodes: 0"),
        "expected `episodes: 0`, got: {stdout}"
    );
}
