//! End-to-end tests: run the real built `playbook` binary. It is a subcommand
//! CLI whose `inject` subcommand is the UserPromptSubmit hook (runs under
//! run_hook → must always exit 0, never breaking a turn). All runs use an
//! isolated temp HOME + CWD so the real `~/.playbook` store is never touched.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// A unique throwaway directory used as both HOME and CWD for one test.
fn temp_sandbox(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "playbook-{tag}-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run the binary with `args` in an isolated HOME/CWD. Returns (exit_code, stdout).
fn run(args: &[&str], tag: &str) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_playbook");
    let sandbox = temp_sandbox(tag);
    let out = Command::new(bin)
        .args(args)
        .current_dir(&sandbox)
        .env("HOME", &sandbox)
        .output()
        .expect("binary runs");
    std::fs::remove_dir_all(&sandbox).ok();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

/// Feed `payload` on stdin to `args` in an isolated HOME/CWD. Returns (exit_code, stdout).
fn run_with_stdin(args: &[&str], payload: &str, tag: &str) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_playbook");
    let sandbox = temp_sandbox(tag);
    let mut child = Command::new(bin)
        .args(args)
        .current_dir(&sandbox)
        .env("HOME", &sandbox)
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
    std::fs::remove_dir_all(&sandbox).ok();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn version_prints_a_number() {
    let (code, stdout) = run(&["--version"], "version");
    assert_eq!(code, 0, "--version must exit 0");
    assert!(
        stdout.contains("playbook"),
        "expected the binary name in version output, got: {stdout}"
    );
}

#[test]
fn list_on_empty_store_reports_no_notes() {
    // Empty HOME/CWD → no notes visible → the documented hint line.
    let (code, stdout) = run(&["list"], "list");
    assert_eq!(code, 0, "list must exit 0");
    assert!(
        stdout.contains("(no notes"),
        "expected the no-notes hint, got: {stdout}"
    );
}

#[test]
fn inject_hook_survives_empty_stdin() {
    // inject runs under run_hook: empty stdin must never break a turn (exit 0),
    // and with no parseable input it stays silent.
    let (code, stdout) = run_with_stdin(&["inject"], "", "inject-empty");
    assert_eq!(code, 0, "inject must exit 0 on empty stdin");
    assert!(
        stdout.trim().is_empty(),
        "inject must stay silent on empty stdin, got: {stdout}"
    );
}

#[test]
fn inject_hook_survives_garbage_stdin() {
    let (code, stdout) = run_with_stdin(&["inject"], "not json", "inject-garbage");
    assert_eq!(code, 0, "inject must exit 0 on garbage stdin");
    assert!(
        stdout.trim().is_empty(),
        "inject must stay silent on garbage stdin, got: {stdout}"
    );
}
