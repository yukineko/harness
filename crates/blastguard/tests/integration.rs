//! End-to-end tests: feed a PreToolUse payload to the real built binary over
//! stdin and assert on stdout + exit code.

use std::io::Write;
use std::process::{Command, Stdio};

/// Run the binary with `payload` on stdin. Returns (exit_code, stdout).
fn run(payload: &str) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_blastguard");
    let mut child = Command::new(bin)
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
fn bash_rm_rf_is_denied() {
    let payload = r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"rm -rf build"}}"#;
    let (code, stdout) = run(payload);
    assert_eq!(code, 0, "hook must always exit 0");
    assert!(
        stdout.contains(r#""permissionDecision":"deny""#),
        "expected deny, got stdout: {stdout}"
    );
}

#[test]
fn write_empty_overwrite_is_denied() {
    let payload = r#"{"hook_event_name":"PreToolUse","tool_name":"Write","tool_input":{"file_path":"src/main.rs","content":""}}"#;
    let (code, stdout) = run(payload);
    assert_eq!(code, 0);
    assert!(
        stdout.contains(r#""permissionDecision":"deny""#),
        "expected deny, got stdout: {stdout}"
    );
}

#[test]
fn config_file_edit_is_allowed_silently() {
    let payload = r#"{"hook_event_name":"PreToolUse","tool_name":"Edit","tool_input":{"file_path":".claude/settings.json"}}"#;
    let (code, stdout) = run(payload);
    assert_eq!(code, 0);
    assert!(
        stdout.trim().is_empty(),
        "allow must be silent, got: {stdout}"
    );
}

#[test]
fn benign_command_is_allowed_silently() {
    let payload = r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"cargo test"}}"#;
    let (code, stdout) = run(payload);
    assert_eq!(code, 0);
    assert!(
        stdout.trim().is_empty(),
        "allow must be silent, got: {stdout}"
    );
}

#[test]
fn empty_stdin_is_silent() {
    let (code, stdout) = run("");
    assert_eq!(code, 0);
    assert!(
        stdout.trim().is_empty(),
        "empty stdin must be silent, got: {stdout}"
    );
}

#[test]
fn config_file_delete_via_bash_is_allowed() {
    let payload = r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"rm package.json"}}"#;
    let (code, stdout) = run(payload);
    assert_eq!(code, 0);
    assert!(
        stdout.trim().is_empty(),
        "allow must be silent, got: {stdout}"
    );
}
