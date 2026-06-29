//! End-to-end tests for the real built `tracekit` binary.
//!
//! tracekit is a plain clap CLI (NOT a lifecycle hook): record / trace / export
//! / list, file-only with no network. We isolate HOME so spans are written under
//! a throwaway `~/.tracekit` and never touch real state. Determinism comes from
//! `--end-unix-ms`, which overrides the wall-clock timestamp.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

/// Run the binary with `args`, inside an isolated HOME. Returns (exit, stdout).
fn run_in(home: &PathBuf, args: &[&str]) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_tracekit");
    let out = Command::new(bin)
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn unique_home() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let id = N.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("tracekit-it-{}-{}", std::process::id(), id));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn version_runs() {
    let home = unique_home();
    let (code, stdout) = run_in(&home, &["--version"]);
    assert_eq!(code, 0, "--version must exit 0");
    assert!(
        stdout.contains("tracekit"),
        "expected the binary name in --version, got: {stdout}"
    );
}

#[test]
fn list_with_no_runs_exits_zero() {
    // `list` is read-only and tolerates a missing store directory (exit 0).
    let home = unique_home();
    let (code, _stdout) = run_in(&home, &["list"]);
    assert_eq!(code, 0, "list must exit 0 even with no recorded runs");
}

#[test]
fn record_then_trace_round_trips() {
    let home = unique_home();
    // Record one root span with a fixed end timestamp for determinism.
    let (rc, _) = run_in(
        &home,
        &[
            "record",
            "--run",
            "RID1",
            "--span",
            "root",
            "--name",
            "interp",
            "--phase",
            "interpreter",
            "--ms",
            "10",
            "--status",
            "ok",
            "--end-unix-ms",
            "1700000000000",
        ],
    );
    assert_eq!(rc, 0, "record must succeed");

    // The run should now show up in `list`.
    let (lc, list_out) = run_in(&home, &["list"]);
    assert_eq!(lc, 0);
    assert!(
        list_out.contains("RID1"),
        "recorded run should be listed, got: {list_out}"
    );

    // And `trace` should render its span tree (stdout, exit 0).
    let (tc, trace_out) = run_in(&home, &["trace", "RID1"]);
    assert_eq!(tc, 0, "trace must exit 0 for a recorded run");
    assert!(
        trace_out.contains("interp") || trace_out.contains("RID1"),
        "trace output should mention the span/run, got: {trace_out}"
    );
}
