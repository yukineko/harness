//! End-to-end tests: drive the real built `backlog` binary and assert on exit
//! code + stdout. `backlog` is a subcommand CLI; `session-start` is a hook whose
//! invariant is to always exit 0 (never break a turn).

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// A unique, isolated temp HOME so `~/.backlog` (the task store) is fresh and the
/// real queue is never read or written.
fn temp_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "backlog-it-{}-{}-{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run `backlog <args>` with `payload` on stdin under an isolated HOME.
fn run(args: &[&str], payload: &str, home: &PathBuf) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_backlog");
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
fn help_lists_the_about_line() {
    let home = temp_home("help");
    let (code, stdout) = run(&["--help"], "", &home);
    assert_eq!(code, 0, "--help must exit 0");
    assert!(
        stdout.contains("Cross-project task queue"),
        "expected the about line, got: {stdout}"
    );
}

#[test]
fn list_on_empty_store_says_no_tasks() {
    // Read-only subcommand against a fresh, isolated store.
    let home = temp_home("list");
    let (code, stdout) = run(&["list"], "", &home);
    assert_eq!(code, 0, "list on an empty store must succeed");
    assert!(
        stdout.contains("no tasks"),
        "empty store should report 'no tasks', got: {stdout}"
    );
}

#[test]
fn list_json_emits_machine_readable_array() {
    // The contract autoflow depends on: `list --json` prints a JSON array whose
    // tasks carry `title` and `status`. Add one task, then read it back as JSON.
    let home = temp_home("list-json");
    let (add_code, _) = run(
        &[
            "add",
            "--title",
            "JSON task",
            "--project",
            "/p",
            "--priority",
            "p1",
        ],
        "",
        &home,
    );
    assert_eq!(add_code, 0, "add must succeed");

    let (code, stdout) = run(
        &["list", "--project", "/p", "--status", "pending", "--json"],
        "",
        &home,
    );
    assert_eq!(code, 0, "list --json must succeed");
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("list --json must emit valid JSON ({e}): {stdout}"));
    let arr = v.as_array().expect("top-level must be an array");
    assert_eq!(arr.len(), 1, "exactly the one pending task");
    assert_eq!(arr[0]["title"], "JSON task");
    assert_eq!(arr[0]["status"], "pending");
}

#[test]
fn lock_status_reports_none_when_unheld() {
    let home = temp_home("lock");
    let (code, stdout) = run(&["lock", "status"], "", &home);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("none"),
        "no lock held → 'none', got: {stdout}"
    );
}

#[test]
fn session_start_with_valid_payload_exits_zero() {
    // SessionStart hook with a well-formed payload; fresh store → no pending
    // tasks → no additionalContext, but it must exit 0 cleanly.
    let home = temp_home("ss-valid");
    let payload = r#"{"hook_event_name":"SessionStart","session_id":"it-1","cwd":"/tmp"}"#;
    let (code, _stdout) = run(&["session-start"], payload, &home);
    assert_eq!(code, 0, "SessionStart hook must always exit 0");
}

#[test]
fn session_start_with_malformed_stdin_exits_zero() {
    // Fail-soft invariant: malformed stdin is skipped (logged to stderr), hook
    // still exits 0 and never breaks the turn.
    let home = temp_home("ss-bad");
    let (code, stdout) = run(&["session-start"], "not json", &home);
    assert_eq!(code, 0, "malformed stdin must still exit 0");
    assert!(stdout.trim().is_empty(), "got: {stdout}");
}
