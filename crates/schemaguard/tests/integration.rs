//! End-to-end tests for the schemaguard binary.
//!
//! schemaguard is a plain 0/1/2 gate CLI (NOT a lifecycle hook):
//!   0 — JSON parsed and schema valid
//!   1 — JSON parsed but schema violations found
//!   2 — JSON failed to parse, OR an unknown schema was requested
//! `list` is the safe read-only subcommand.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_home() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("schemaguard-it-{}-{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Run `schemaguard <args>` with `payload` on stdin in an isolated HOME (so the
/// reject-metrics store at ~/.schemaguard is never the real one).
fn run(args: &[&str], payload: &str) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_schemaguard");
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
fn help_describes_the_gate() {
    let (code, stdout) = run(&["--help"], "");
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Schema-validation gate"),
        "expected about string, got: {stdout}"
    );
}

#[test]
fn list_prints_known_schema_names() {
    let (code, stdout) = run(&["list"], "");
    assert_eq!(code, 0);
    assert!(
        stdout.contains("decomposition") && stdout.contains("episode"),
        "expected known schema names, got: {stdout}"
    );
}

#[test]
fn check_valid_decomposition_exits_zero() {
    let payload = r#"{"goal":"ship it","tasks":[{"id":"t1","title":"do","class":"serial","done_criteria":"done"}]}"#;
    let (code, stdout) = run(&["check", "--schema", "decomposition"], payload);
    assert_eq!(code, 0, "valid JSON + valid schema must exit 0");
    assert!(
        stdout.contains("\"valid\":true"),
        "expected valid:true, got: {stdout}"
    );
}

#[test]
fn check_schema_violation_exits_one() {
    // Missing the required `tasks` field → schema violation, exit 1.
    let payload = r#"{"goal":"ship it"}"#;
    let (code, stdout) = run(&["check", "--schema", "decomposition"], payload);
    assert_eq!(code, 1, "schema violations must exit 1");
    assert!(
        stdout.contains("\"valid\":false"),
        "expected valid:false, got: {stdout}"
    );
}

#[test]
fn check_unknown_schema_exits_two() {
    let (code, _stdout) = run(&["check", "--schema", "no-such-schema"], "{}");
    assert_eq!(code, 2, "unknown schema must exit 2");
}

#[test]
fn check_unparseable_json_exits_two() {
    let (code, stdout) = run(&["check", "--schema", "decomposition"], "not json");
    assert_eq!(code, 2, "unparseable JSON must exit 2");
    assert!(
        stdout.contains("invalid JSON"),
        "expected parse-error message, got: {stdout}"
    );
}
