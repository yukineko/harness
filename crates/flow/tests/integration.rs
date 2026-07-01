//! End-to-end tests for the `flow` binary. `flow` is a thin, subscription-native
//! scaffold whose only subcommand is the SessionStart `propose` hook; it never
//! touches the filesystem, so we just spawn the real binary and assert on stdout
//! + exit code.

use std::process::Command;

/// Run the binary with `args` (no stdin needed). Returns (exit_code, stdout).
fn run(args: &[&str]) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_flow");
    let out = Command::new(bin).args(args).output().expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn help_lists_propose_subcommand() {
    let (code, stdout) = run(&["--help"]);
    assert_eq!(code, 0, "--help must exit 0");
    // clap renders the about string and the `propose` subcommand in --help.
    assert!(
        stdout.contains("propose"),
        "expected the propose subcommand in --help, got: {stdout}"
    );
}

#[test]
fn version_flag_exits_zero() {
    // `#[command(version)]` is set, so --version is supported.
    let (code, _stdout) = run(&["--version"]);
    assert_eq!(code, 0, "--version must exit 0");
}

#[test]
fn propose_emits_flow_directive_when_backlog_unavailable() {
    // `propose` is state-aware: it queries the backlog and stays silent when the
    // queue is empty. To test the directive deterministically we make the backlog
    // binary unreachable (empty PATH), which forces the static fallback directive.
    // That fallback must carry the `[flow]` / `/flow` tags and still exit 0
    // (the hook never breaks a turn).
    let bin = env!("CARGO_BIN_EXE_flow");
    let out = Command::new(bin)
        .arg("propose")
        .env("PATH", "")
        .output()
        .expect("binary runs");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(code, 0, "propose must exit 0 — never breaks the turn");
    assert!(
        stdout.contains("[flow]") && stdout.contains("/flow"),
        "expected the [flow] fallback directive, got: {stdout}"
    );
}
