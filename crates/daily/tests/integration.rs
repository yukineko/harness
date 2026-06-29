//! End-to-end tests for `daily`: a clap CLI whose `session-start` subcommand is a
//! SessionStart hook (must NEVER break a turn → always exit 0). Every test pins
//! HOME to an isolated temp dir so the hook's `~/.daily/state` writes can't touch
//! the real home, and so output never depends on the real environment.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Run `daily <args...>` with `home` as $HOME and `stdin` piped in.
/// Returns (exit_code, stdout).
fn run_with_home(home: &Path, args: &[&str], stdin: &str) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_daily");
    let mut child = Command::new(bin)
        .args(args)
        .env("HOME", home)
        .current_dir(home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary spawns");
    if let Some(mut child_stdin) = child.stdin.take() {
        let _ = child_stdin.write_all(stdin.as_bytes());
    }
    let out = child.wait_with_output().expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

/// A unique empty temp dir to serve as an isolated $HOME.
fn temp_home(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("daily-it-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp home");
    dir
}

#[test]
fn help_lists_real_subcommands() {
    let home = temp_home("help");
    let (code, stdout) = run_with_home(&home, &["--help"], "");
    assert_eq!(code, 0);
    assert!(stdout.contains("Usage: daily"), "got: {stdout}");
    assert!(stdout.contains("session-start"), "got: {stdout}");
    assert!(stdout.contains("install"), "got: {stdout}");
}

#[test]
fn session_start_valid_payload_exits_0() {
    // A valid SessionStart payload: whether or not cargo-deny exists on PATH, the
    // run_hook wrapper guarantees exit 0 (the hook must never break a turn).
    let home = temp_home("valid");
    let payload = format!(
        r#"{{"hook_event_name":"SessionStart","cwd":"{}"}}"#,
        home.display()
    );
    let (code, _stdout) = run_with_home(&home, &["session-start"], &payload);
    assert_eq!(code, 0, "SessionStart hook must always exit 0");
}

#[test]
fn session_start_empty_stdin_exits_0() {
    // Empty stdin: HookInput::parse falls back to default; still must exit 0.
    let home = temp_home("empty");
    let (code, _stdout) = run_with_home(&home, &["session-start"], "");
    assert_eq!(code, 0, "empty stdin must not break the turn");
}

#[test]
fn session_start_malformed_stdin_exits_0() {
    // Garbage payload: fail-soft, never breaks the turn.
    let home = temp_home("bad");
    let (code, _stdout) = run_with_home(&home, &["session-start"], "not json");
    assert_eq!(code, 0, "malformed stdin must not break the turn");
}
