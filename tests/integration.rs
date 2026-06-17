//! End-to-end tests driving the built binary against a throwaway git repo with
//! a *fake* agent (a `bash -c` script), so no real LLM is required. Exercises
//! scope resolution, prompt delivery, marker parsing, and report/sentinel I/O.

use std::fs;
use std::path::Path;
use std::process::Command;

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

fn init_repo(repo: &Path) -> String {
    git(repo, &["init", "-q"]);
    git(repo, &["config", "user.email", "t@t.t"]);
    git(repo, &["config", "user.name", "t"]);
    git(repo, &["config", "commit.gpgsign", "false"]);
    fs::write(repo.join("README.md"), "seed\n").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "seed"]);
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Write a config whose "agent" is a bash script that drains stdin and prints
/// `body` verbatim.
fn write_config(repo: &Path, agent_output: &str) {
    // Embed the canned output via a heredoc-free printf-safe single-quoted arg.
    let script = format!("cat >/dev/null; cat <<'SPECGUARD_EOF'\n{agent_output}\nSPECGUARD_EOF");
    let cfg = format!(
        r#"
[project]
name = "Demo"
root = "."

[agent]
command = "bash"
args = ["-c", {script:?}]

[output]
report_dir = "reports"
sentinel = ".pending"

[[area]]
name = "src"
globs = ["src/**"]
canon = ["docs/spec.md"]
"#,
    );
    fs::write(repo.join("specguard.toml"), cfg).unwrap();
}

fn run_specguard(repo: &Path, baseline: &str, sub: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_specguard"))
        .current_dir(repo)
        .args(["--config", "specguard.toml", "--baseline", baseline, "--date", "2026-01-01"])
        .args(sub)
        .output()
        .expect("specguard runs")
}

#[test]
fn run_with_findings_writes_report_and_sentinel() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let base = init_repo(repo);

    // A change inside the "src" area so it lands in scope.
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/main.rs").as_path(), "fn main() {}\n").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "add src"]);

    write_config(
        repo,
        "# Demo audit\n\nfinding body\n\n<<<SPEC_AUDIT>>>\nneeds_user: yes\nsummary: fix the drift",
    );

    let out = run_specguard(repo, &base, &["run"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let report = fs::read_to_string(repo.join("reports/2026-01-01.md")).unwrap();
    assert!(report.contains("Demo audit"));
    assert!(report.contains("finding body"));
    // Trailer must be stripped from the saved report.
    assert!(!report.contains("<<<SPEC_AUDIT>>>"));

    let sentinel = fs::read_to_string(repo.join(".pending")).unwrap();
    assert!(sentinel.contains("summary: fix the drift"));
    assert!(sentinel.contains("report: reports/2026-01-01.md"));

    // Findings hold the baseline (not advanced) until `ack`, so the same drift is
    // re-detected on the next run.
    assert!(
        !repo.join("reports/.last-ref").exists(),
        "baseline must be held while a sentinel is pending"
    );
}

#[test]
fn run_without_findings_writes_no_sentinel() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let base = init_repo(repo);
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/x.rs"), "//\n").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "x"]);

    write_config(repo, "# clean\n\n<<<SPEC_AUDIT>>>\nneeds_user: no\nsummary: なし");
    let out = run_specguard(repo, &base, &["run"]);
    assert!(out.status.success());
    assert!(repo.join("reports/2026-01-01.md").exists());
    assert!(!repo.join(".pending").exists(), "no sentinel when no findings");
    // A fully clean run advances the baseline.
    assert!(repo.join("reports/.last-ref").exists(), "clean run advances baseline");
}

#[test]
fn missing_marker_exits_3_and_no_sentinel() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let base = init_repo(repo);
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/x.rs"), "//\n").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "x"]);

    write_config(repo, "# report with no trailer at all");
    let out = run_specguard(repo, &base, &["run"]);
    assert_eq!(out.status.code(), Some(3));
    assert!(!repo.join(".pending").exists());
    // Raw report still saved for inspection.
    assert!(repo.join("reports/2026-01-01.md").exists());
}

#[test]
fn agent_nonzero_exit_maps_to_4_with_true_code_on_stderr() {
    // An agent failure maps to specguard's reserved EXIT_AGENT_FAILED (4), never
    // the raw code (which could collide with 2=usage / 3=no-marker). The real
    // code is surfaced on stderr. No report or sentinel is written.
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let base = init_repo(repo);
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/x.rs"), "//\n").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "x"]);

    let cfg = r#"
[project]
name = "Demo"
root = "."
[agent]
command = "bash"
args = ["-c", "cat >/dev/null; exit 3"]
[output]
report_dir = "reports"
sentinel = ".pending"
[[area]]
name = "src"
globs = ["src/**"]
"#;
    fs::write(repo.join("specguard.toml"), cfg).unwrap();

    let out = run_specguard(repo, &base, &["run"]);
    assert_eq!(out.status.code(), Some(4), "agent failure -> EXIT_AGENT_FAILED");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("code 3"), "true agent code on stderr: {stderr}");
    assert!(!repo.join(".pending").exists());
    assert!(!repo.join("reports/2026-01-01.md").exists(), "no report on agent failure");
}

#[test]
fn ack_clears_the_sentinel() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let base = init_repo(repo);
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/main.rs").as_path(), "fn main() {}\n").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "add src"]);

    write_config(
        repo,
        "# Demo audit\n\nfinding body\n\n<<<SPEC_AUDIT>>>\nneeds_user: yes\nsummary: fix the drift",
    );
    let out = run_specguard(repo, &base, &["run"]);
    assert!(out.status.success());
    assert!(repo.join(".pending").exists(), "sentinel raised");

    let out = run_specguard(repo, &base, &["ack"]);
    assert!(out.status.success());
    assert!(!repo.join(".pending").exists(), "ack removed the sentinel");
}

#[test]
fn pending_sentinel_holds_baseline_until_ack() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let base = init_repo(repo);
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/main.rs").as_path(), "fn main() {}\n").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "add src"]);

    // 1. Findings run: sentinel raised, baseline held.
    write_config(repo, "# audit\n\n<<<SPEC_AUDIT>>>\nneeds_user: yes\nsummary: drift");
    assert!(run_specguard(repo, &base, &["run"]).status.success());
    assert!(repo.join(".pending").exists());
    assert!(!repo.join("reports/.last-ref").exists(), "held on findings");

    // 2. Clean run while the sentinel is still pending: baseline stays held,
    //    sentinel left untouched.
    write_config(repo, "# clean\n\n<<<SPEC_AUDIT>>>\nneeds_user: no\nsummary: なし");
    assert!(run_specguard(repo, &base, &["run"]).status.success());
    assert!(repo.join(".pending").exists(), "sentinel untouched while pending");
    assert!(!repo.join("reports/.last-ref").exists(), "still held pre-ack");

    // 3. After ack, a clean run advances the baseline.
    assert!(run_specguard(repo, &base, &["ack"]).status.success());
    assert!(run_specguard(repo, &base, &["run"]).status.success());
    assert!(repo.join("reports/.last-ref").exists(), "advanced after ack + clean");
}

#[test]
fn scope_subcommand_lists_in_scope_area() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let base = init_repo(repo);
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/x.rs"), "//\n").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "x"]);

    write_config(repo, "unused");
    let out = run_specguard(repo, &base, &["scope"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("in-scope areas:"));
    assert!(stdout.contains("- src (1 file(s))"));
}

#[test]
fn unresolvable_baseline_falls_back_to_all_tracked() {
    // Young repo (1 commit) with a bogus baseline and a non-existent fallback:
    // specguard should audit all tracked files instead of hard-erroring.
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo(repo);
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/x.rs"), "//\n").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "x"]);

    write_config(repo, "unused");
    let out = run_specguard(repo, "does-not-exist-ref", &["scope"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("- src"), "src area should be in scope via all-tracked fallback:\n{stdout}");
}

#[test]
fn prompt_subcommand_renders_without_agent() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let base = init_repo(repo);
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/x.rs"), "//\n").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "x"]);

    write_config(repo, "unused");
    let out = run_specguard(repo, &base, &["prompt"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Demo"));
    assert!(stdout.contains("docs/spec.md"));
    assert!(stdout.contains("<<<SPEC_AUDIT>>>"));
    assert!(!stdout.contains("{{"));
}

// --- Parallel fan-out: two areas + an invariant => three shards audited in
// separate processes and merged. ---

/// Commit a file in each of two areas (`alpha`, `beta`) so both land in scope.
fn commit_two_areas(repo: &Path) {
    fs::create_dir_all(repo.join("alpha")).unwrap();
    fs::create_dir_all(repo.join("beta")).unwrap();
    fs::write(repo.join("alpha/a.rs"), "//\n").unwrap();
    fs::write(repo.join("beta/b.rs"), "//\n").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "two areas"]);
}

/// Config with two areas + one invariant (=> 3 shards) and a CONTENT-SENSITIVE
/// fake agent: `script` (bash -c) inspects the per-shard prompt on stdin, so we
/// can give each shard a distinct verdict and prove each got its own prompt.
fn write_fanout_config(repo: &Path, script: &str) {
    let cfg = format!(
        r#"
[project]
name = "Demo"
root = "."
[agent]
command = "bash"
args = ["-c", {script:?}]
[output]
report_dir = "reports"
sentinel = ".pending"
[[area]]
name = "alpha"
globs = ["alpha/**"]
[[area]]
name = "beta"
globs = ["beta/**"]
[[invariant]]
name = "inv1"
description = "some rule"
"#,
    );
    fs::write(repo.join("specguard.toml"), cfg).unwrap();
}

#[test]
fn fanout_merges_shards_and_ors_needs_user() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let base = init_repo(repo);
    commit_two_areas(repo);

    // Only the `beta` shard flags needs_user; alpha and the invariant shard are
    // clean. The agent routes on the shard-scope line of each prompt.
    let script = r#"input=$(cat)
if printf '%s' "$input" | grep -q '領域「alpha」'; then
  printf '# alpha audit\n\n<<<SPEC_AUDIT>>>\nneeds_user: no\nsummary: なし\n'
elif printf '%s' "$input" | grep -q '領域「beta」'; then
  printf '# beta audit\n\n<<<SPEC_AUDIT>>>\nneeds_user: yes\nsummary: beta drift\n'
else
  printf '# inv audit\n\n<<<SPEC_AUDIT>>>\nneeds_user: no\nsummary: なし\n'
fi"#;
    write_fanout_config(repo, script);

    let out = run_specguard(repo, &base, &["run"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let report = fs::read_to_string(repo.join("reports/2026-01-01.md")).unwrap();
    // All three shards are present and merged (each got its own focused prompt).
    assert!(report.contains("## shard: alpha"), "report:\n{report}");
    assert!(report.contains("## shard: beta"), "report:\n{report}");
    assert!(report.contains("## shard: invariants"), "report:\n{report}");
    assert!(report.contains("alpha audit") && report.contains("beta audit"));
    // Every shard's trailer is stripped from the merged report.
    assert!(!report.contains("<<<SPEC_AUDIT>>>"));

    // needs_user is OR'd across shards: beta alone flagged -> sentinel raised.
    // Exactly one flagged shard -> summary is verbatim (no label prefix).
    let sentinel = fs::read_to_string(repo.join(".pending")).unwrap();
    assert!(sentinel.contains("summary: beta drift"), "sentinel:\n{sentinel}");
    // Findings pending -> baseline held.
    assert!(!repo.join("reports/.last-ref").exists());
}

#[test]
fn fanout_labels_summary_when_multiple_shards_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let base = init_repo(repo);
    commit_two_areas(repo);

    // Both area shards flag -> the merged summary labels each contribution.
    let script = r#"input=$(cat)
if printf '%s' "$input" | grep -q '領域「alpha」'; then
  printf '# alpha audit\n\n<<<SPEC_AUDIT>>>\nneeds_user: yes\nsummary: alpha drift\n'
elif printf '%s' "$input" | grep -q '領域「beta」'; then
  printf '# beta audit\n\n<<<SPEC_AUDIT>>>\nneeds_user: yes\nsummary: beta drift\n'
else
  printf '# inv audit\n\n<<<SPEC_AUDIT>>>\nneeds_user: no\nsummary: なし\n'
fi"#;
    write_fanout_config(repo, script);

    let out = run_specguard(repo, &base, &["run"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let sentinel = fs::read_to_string(repo.join(".pending")).unwrap();
    assert!(sentinel.contains("[alpha] alpha drift"), "sentinel:\n{sentinel}");
    assert!(sentinel.contains("[beta] beta drift"), "sentinel:\n{sentinel}");
}
