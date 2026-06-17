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

    // .last-ref advanced to HEAD.
    assert!(repo.join("reports/.last-ref").exists());
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
