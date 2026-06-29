//! End-to-end tests for the real built `evalkit` binary. evalkit is a plain
//! clap subcommand CLI (NOT a hook): `run`/`list` exit 0 (all passed), 1 (a
//! regression), or 2 (harness error — no cases / unreadable eval files).

use std::process::Command;

/// Run the binary with `args`. Returns (exit_code, stdout, stderr).
fn run(args: &[&str]) -> (i32, String, String) {
    let bin = env!("CARGO_BIN_EXE_evalkit");
    let out = Command::new(bin).args(args).output().expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn help_exits_zero_and_describes_the_harness() {
    let (code, stdout, _) = run(&["--help"]);
    assert_eq!(code, 0, "--help must exit 0");
    assert!(
        stdout.contains("Offline golden-regression eval harness"),
        "expected the about string, got: {stdout}"
    );
}

#[test]
fn version_exits_zero() {
    let (code, stdout, _) = run(&["--version"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("evalkit"), "got: {stdout}");
}

#[test]
fn run_against_empty_dir_is_a_harness_error() {
    // An eval dir with no *.jsonl cases is a *harness* error (exit 2), distinct
    // from a real regression (exit 1) — the documented exit-code contract.
    let dir = std::env::temp_dir().join(format!("evalkit-it-empty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (code, _stdout, stderr) = run(&["run", "--dir", dir.to_str().unwrap()]);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(code, 2, "no cases found must exit 2 (harness error)");
    assert!(
        stderr.contains("no cases found"),
        "expected the no-cases message, got stderr: {stderr}"
    );
}
