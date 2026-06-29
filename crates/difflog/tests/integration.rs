//! End-to-end tests: invoke the real built `difflog` binary and assert on
//! stdout + exit code. difflog is a clap subcommand CLI (SessionStart /
//! SessionEnd are hooks; List / Last / Status / Init / Install are CLI verbs).

use std::process::Command;

/// Run the binary with `args` under an isolated HOME so it never reads or
/// writes the real `~/.difflog`. Returns (exit_code, stdout).
fn run(args: &[&str]) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_difflog");
    let home = std::env::temp_dir().join(format!("difflog-it-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let out = Command::new(bin)
        .args(args)
        .env("HOME", &home)
        .output()
        .expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn help_exits_zero_and_describes_the_tool() {
    let (code, stdout) = run(&["--help"]);
    assert_eq!(code, 0, "--help must exit 0");
    assert!(
        stdout.contains("Session diff-log"),
        "expected the about string, got: {stdout}"
    );
}

#[test]
fn version_exits_zero() {
    let (code, stdout) = run(&["--version"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("difflog"), "got: {stdout}");
}

#[test]
fn status_prints_resolved_config() {
    // `status` is read-only: it loads the (default) config and prints it.
    let (code, stdout) = run(&["status"]);
    assert_eq!(code, 0, "status must exit 0");
    assert!(stdout.contains("enabled:"), "got: {stdout}");
    assert!(stdout.contains("log_dir:"), "got: {stdout}");
    assert!(stdout.contains("diff_body_limit:"), "got: {stdout}");
}
