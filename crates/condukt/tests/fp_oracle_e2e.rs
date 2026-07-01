//! End-to-end coverage for the Fail→Pass reproduction gate, exercised through
//! the REAL built `condukt` binary against a real (spawned) `tdd` binary.
//!
//! `condukt state set --status verified` must:
//!   * ACCEPT a `kind:"fix"` task whose RED/GREEN proofs are a genuine
//!     Fail→Pass transition (`tdd oracle` reports `valid_fp_oracle:true`).
//!   * REJECT (nonzero exit, refusal message) a `kind:"fix"` task whose
//!     proofs do NOT form a valid Fail→Pass transition.
//!   * ACCEPT unconditionally (legacy path) a task with no `kind` (or a
//!     `kind` outside fix/feature) — the oracle gate never applies to it.
//!
//! `condukt` has no Cargo dependency on the `tdd` crate, so
//! `env!("CARGO_BIN_EXE_tdd")` is not available in this test binary. Instead
//! `tdd_bin_dir` locates (building if necessary) a sibling `tdd` binary in the
//! same target profile directory this condukt test binary itself was built
//! into, and that directory is prepended to `PATH` for every spawned
//! `condukt` child process — mirroring how `oracle::check_oracle` shells out
//! to `tdd oracle --task <id>` via a bare `Command::new("tdd")` lookup.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

fn unique_dir(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let id = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "condukt-fp-e2e-{tag}-{}-{}",
        std::process::id(),
        id
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create isolated dir");
    dir
}

/// Locate a `tdd` binary usable by the spawned `condukt` process, building it
/// (once, via `cargo build -p tdd --bin tdd`) if it isn't already sitting next
/// to this condukt test binary in the target profile directory. Returns the
/// directory to prepend to `PATH` (not the binary path itself).
fn tdd_bin_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe of this test binary");
    let deps_dir = exe.parent().expect("deps dir").to_path_buf();
    let profile_dir = deps_dir.parent().expect("profile dir").to_path_buf();
    let bin_name = if cfg!(windows) { "tdd.exe" } else { "tdd" };
    let bin_path = profile_dir.join(bin_name);
    if !bin_path.exists() {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
        let is_release = profile_dir.file_name().and_then(|s| s.to_str()) == Some("release");
        let mut cmd = Command::new(&cargo);
        cmd.args(["build", "-p", "tdd", "--bin", "tdd"]);
        if is_release {
            cmd.arg("--release");
        }
        let status = cmd.status().expect("spawning `cargo build -p tdd`");
        assert!(status.success(), "`cargo build -p tdd` failed");
    }
    assert!(
        bin_path.exists(),
        "expected a tdd binary at {} after build",
        bin_path.display()
    );
    profile_dir
}

/// Run the real `condukt` binary with `args`, in `dir`, with `home` as its
/// `$HOME` (isolating `~/.condukt/state`) and `extra_path` prepended to
/// `$PATH` so a spawned `tdd oracle` resolves to our sibling test build.
/// Returns `(exit_code, stdout, stderr)`.
fn run_condukt(dir: &Path, home: &Path, extra_path: &Path, args: &[&str]) -> (i32, String, String) {
    let bin = env!("CARGO_BIN_EXE_condukt");
    let existing_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![extra_path.to_path_buf()];
    paths.extend(std::env::split_paths(&existing_path));
    let new_path = std::env::join_paths(paths).expect("join PATH");

    let out = Command::new(bin)
        .args(args)
        .current_dir(dir)
        .env("HOME", home)
        .env("PATH", new_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("condukt spawns");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Build a minimal single-task decomposition JSON. `kind`/`repro` are
/// optional passthrough fields (`None` omits the field entirely, exercising
/// the back-compat "no kind" path).
fn decomposition_json(task_id: &str, kind: Option<&str>, repro: Option<&str>) -> String {
    let mut task = serde_json::json!({ "id": task_id, "title": "demo task" });
    if let Some(k) = kind {
        task["kind"] = serde_json::Value::String(k.to_string());
    }
    if let Some(r) = repro {
        task["reproduction_tests"] = serde_json::Value::String(r.to_string());
    }
    serde_json::json!({ "goal": "demo goal", "tasks": [task] }).to_string()
}

/// Write a `<task>.<kind>.json` RED/GREEN proof artifact — the shape
/// `tdd red`/`tdd green` produce, reduced to the one field `tdd oracle`
/// reads (`passed`) — under `<dir>/.tdd/`, `tdd`'s default `proof_dir`. This
/// is exactly the directory `tdd oracle` will read from when `condukt`
/// shells out to it with `current_dir(run_dir)`, and `run_dir` defaults to
/// the task's `condukt state set` invocation cwd when no worktree is
/// recorded — i.e. `dir` itself.
fn write_proof(dir: &Path, task: &str, kind: &str, passed: bool) {
    let proof_dir = dir.join(".tdd");
    std::fs::create_dir_all(&proof_dir).expect("create proof dir");
    let path = proof_dir.join(format!("{task}.{kind}.json"));
    std::fs::write(&path, serde_json::json!({ "passed": passed }).to_string())
        .expect("write proof artifact");
}

/// Seed a fresh isolated run: writes the decomposition to `dir`, runs
/// `condukt state init`, and returns the run id.
fn init_run(
    dir: &Path,
    home: &Path,
    tdd_path: &Path,
    run_id: &str,
    task_id: &str,
    kind: Option<&str>,
    repro: Option<&str>,
) {
    let dec_path = dir.join("decomposition.json");
    std::fs::write(&dec_path, decomposition_json(task_id, kind, repro))
        .expect("write decomposition");
    let (code, stdout, stderr) = run_condukt(
        dir,
        home,
        tdd_path,
        &[
            "state",
            "init",
            "--run",
            run_id,
            "--file",
            dec_path.to_str().unwrap(),
        ],
    );
    assert_eq!(
        code, 0,
        "state init must succeed\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn fix_task_with_valid_fail_to_pass_oracle_is_verified() {
    let dir = unique_dir("accept");
    let home = unique_dir("accept-home");
    let tdd_path = tdd_bin_dir();

    init_run(
        &dir,
        &home,
        &tdd_path,
        "run-accept",
        "task1",
        Some("fix"),
        Some("cargo test -p demo"),
    );

    // A genuine Fail→Pass transition: the test failed before the fix (RED)
    // and passes after it (GREEN).
    write_proof(&dir, "task1", "red", false);
    write_proof(&dir, "task1", "green", true);

    let (code, _stdout, stderr) = run_condukt(
        &dir,
        &home,
        &tdd_path,
        &[
            "state",
            "set",
            "--run",
            "run-accept",
            "--task",
            "task1",
            "--status",
            "verified",
        ],
    );
    assert_eq!(
        code, 0,
        "a genuine F→P oracle must let the task verify\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("1/1 verified"),
        "expected the verified count in stderr, got: {stderr}"
    );
}

#[test]
fn fix_task_without_valid_fail_to_pass_oracle_is_refused() {
    let dir = unique_dir("reject");
    let home = unique_dir("reject-home");
    let tdd_path = tdd_bin_dir();

    init_run(
        &dir,
        &home,
        &tdd_path,
        "run-reject",
        "task1",
        Some("fix"),
        Some("cargo test -p demo"),
    );

    // Fail→Fail: the test failed at RED and STILL fails at GREEN — no proof
    // the fix worked. This must be a genuine, non-fallback reject.
    write_proof(&dir, "task1", "red", false);
    write_proof(&dir, "task1", "green", false);

    let (code, _stdout, stderr) = run_condukt(
        &dir,
        &home,
        &tdd_path,
        &[
            "state",
            "set",
            "--run",
            "run-reject",
            "--task",
            "task1",
            "--status",
            "verified",
        ],
    );
    assert_ne!(
        code, 0,
        "a fix task without a valid F→P oracle must be refused, got success\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("refusing to verify"),
        "expected the refusal message in stderr, got: {stderr}"
    );
    assert!(
        stderr.contains("fail-to-pass"),
        "expected the refusal to name the fail-to-pass oracle, got: {stderr}"
    );
}

#[test]
fn task_without_a_fix_or_feature_kind_verifies_unconditionally() {
    let dir = unique_dir("legacy");
    let home = unique_dir("legacy-home");
    let tdd_path = tdd_bin_dir();

    // No `kind` at all (back-compat path): the oracle gate never applies, so
    // verification must succeed even with zero proof artifacts on disk.
    init_run(&dir, &home, &tdd_path, "run-legacy", "task1", None, None);

    let (code, _stdout, stderr) = run_condukt(
        &dir,
        &home,
        &tdd_path,
        &[
            "state",
            "set",
            "--run",
            "run-legacy",
            "--task",
            "task1",
            "--status",
            "verified",
        ],
    );
    assert_eq!(
        code, 0,
        "a task with no kind must verify unconditionally (legacy path)\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("1/1 verified"),
        "expected the verified count in stderr, got: {stderr}"
    );
}
