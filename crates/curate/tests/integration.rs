//! End-to-end tests for the `curate` subcommand CLI: spawn the real built binary
//! and assert on stdout/stderr + exit code. curate is a plain CLI (not a hook).

use std::path::Path;
use std::process::Command;

/// Run `curate <args...>`. Returns (exit_code, stdout, stderr). curate writes its
/// human status lines to stderr, so tests inspect both streams.
fn run(args: &[&str]) -> (i32, String, String) {
    let bin = env!("CARGO_BIN_EXE_curate");
    let out = Command::new(bin).args(args).output().expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// A unique empty temp dir; used to point `--store` at a guaranteed-missing file.
fn temp_root(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("curate-it-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp root");
    dir
}

#[test]
fn help_lists_real_subcommands() {
    let (code, stdout, _) = run(&["--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Usage: curate"), "got: {stdout}");
    assert!(stdout.contains("candidates"), "got: {stdout}");
    assert!(stdout.contains("promote"), "got: {stdout}");
}

#[test]
fn candidates_on_missing_store_reports_none_and_exits_0() {
    // Read-only subcommand pointed at a store that doesn't exist: the source
    // prints "no playbooks found in <path>" to stderr and returns Ok(()).
    let root = temp_root("cand");
    let store = root.join("does-not-exist.jsonl");
    assert!(!Path::new(&store).exists());
    let (code, _stdout, stderr) = run(&["candidates", "--store", store.to_str().unwrap()]);
    assert_eq!(
        code, 0,
        "empty/missing store is not an error for candidates"
    );
    assert!(stderr.contains("no playbooks found"), "got: {stderr}");
}
