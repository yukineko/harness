//! Case runner: acquire each subject (read file / run cmd), check assertions,
//! aggregate, and report. The assertion core (`check_assert`) is pure over a
//! subject string so it is unit-testable without touching the filesystem or
//! spawning processes.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::json;

use crate::case::{Assert, Case};

/// Where subjects resolve. `root` anchors `file` paths and the cwd of `cmd`
/// subprocesses; `bin_dir` (if set) is prepended to PATH so a freshly-built
/// `target/release` binary can be exercised without installing it.
pub struct RunCfg {
    pub root: PathBuf,
    pub bin_dir: Option<PathBuf>,
}

/// Result of one case: the failures list is empty iff the case passed.
pub struct Outcome {
    pub label: String,
    pub failures: Vec<String>,
}

impl Outcome {
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

/// The acquired subject: text to assert over, plus the exit code for `cmd`
/// subjects (`None` for `file` subjects, which have no exit code).
struct Subject {
    text: String,
    exit: Option<i32>,
}

/// Run one case to an Outcome. Acquisition failures (unreadable file, unspawnable
/// cmd) are themselves recorded as failures so a broken case fails loudly rather
/// than silently passing.
pub fn run_case(case: &Case, cfg: &RunCfg) -> Outcome {
    let mut failures = Vec::new();
    match acquire(case, cfg) {
        Ok(subject) => {
            check_exit(&case.assert, &subject, &mut failures);
            check_assert(&case.assert, &subject.text, &mut failures);
        }
        Err(e) => failures.push(format!("could not acquire subject: {e:#}")),
    }
    Outcome {
        label: case.label(),
        failures,
    }
}

/// Read the `file` or run the `cmd` to obtain the subject text.
fn acquire(case: &Case, cfg: &RunCfg) -> Result<Subject> {
    if let Some(rel) = &case.file {
        let path = cfg.root.join(rel);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        return Ok(Subject { text, exit: None });
    }
    let cmd = case
        .cmd
        .as_ref()
        .context("case has no `file` or `cmd` (should have been caught at parse)")?;
    run_cmd(cmd, case.stdin.as_deref(), cfg)
}

/// Spawn a `cmd` subject and capture stdout + exit code.
fn run_cmd(cmd: &[String], stdin: Option<&str>, cfg: &RunCfg) -> Result<Subject> {
    let (prog, args) = cmd.split_first().context("empty cmd")?;
    let mut c = Command::new(prog);
    c.args(args)
        .current_dir(&cfg.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(bin) = &cfg.bin_dir {
        c.env("PATH", path_with_prefix(bin, &cfg.root)?);
    }
    let mut child = c.spawn().with_context(|| format!("spawning `{prog}`"))?;
    if let Some(input) = stdin {
        use std::io::Write;
        if let Some(mut sink) = child.stdin.take() {
            sink.write_all(input.as_bytes()).ok();
        }
    }
    // Closing stdin (drop above) lets a reader-blocked child finish.
    let out = child.wait_with_output().context("waiting for cmd")?;
    Ok(Subject {
        text: String::from_utf8_lossy(&out.stdout).into_owned(),
        exit: out.status.code(),
    })
}

/// Build a PATH with `bin_dir` (resolved against `root` if relative) prepended.
fn path_with_prefix(bin_dir: &Path, root: &Path) -> Result<std::ffi::OsString> {
    let prefix = if bin_dir.is_absolute() {
        bin_dir.to_path_buf()
    } else {
        root.join(bin_dir)
    };
    let existing = std::env::var_os("PATH").unwrap_or_default();
    let mut parts = vec![prefix];
    parts.extend(std::env::split_paths(&existing));
    std::env::join_paths(parts).context("rebuilding PATH")
}

/// Check the exit-code assertion against a subject.
fn check_exit(a: &Assert, subject: &Subject, failures: &mut Vec<String>) {
    let Some(expected) = a.exit else {
        return;
    };
    match subject.exit {
        Some(actual) if actual == expected => {}
        Some(actual) => failures.push(format!("exit: expected {expected}, got {actual}")),
        None => failures.push("exit asserted on a `file` case (no exit code)".to_string()),
    }
}

/// Pure assertion core: append a human-readable failure for each unmet
/// assertion. Operates only on the subject string so it is fully testable.
pub fn check_assert(a: &Assert, subject: &str, failures: &mut Vec<String>) {
    for s in &a.contains {
        if !subject.contains(s) {
            failures.push(format!("missing required substring {s:?}"));
        }
    }
    for s in &a.not_contains {
        if subject.contains(s) {
            failures.push(format!("forbidden substring {s:?} is present"));
        }
    }
    for pat in &a.regex {
        match Regex::new(pat) {
            Ok(re) if re.is_match(subject) => {}
            Ok(_) => failures.push(format!("regex {pat:?} did not match")),
            Err(e) => failures.push(format!("invalid regex {pat:?}: {e}")),
        }
    }
    for pat in &a.not_regex {
        match Regex::new(pat) {
            Ok(re) if !re.is_match(subject) => {}
            Ok(_) => failures.push(format!("forbidden regex {pat:?} matched")),
            Err(e) => failures.push(format!("invalid regex {pat:?}: {e}")),
        }
    }
}

/// Discover `*.jsonl` golden files under `dir` (sorted) and parse them. Returns
/// `(path, cases)` pairs so reports can attribute a case to its file.
pub fn discover(dir: &Path) -> Result<Vec<(PathBuf, Vec<Case>)>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading eval dir {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let text =
            std::fs::read_to_string(&f).with_context(|| format!("reading {}", f.display()))?;
        let cases = crate::case::parse_jsonl(&text, &f.display().to_string())?;
        out.push((f, cases));
    }
    Ok(out)
}

/// Orchestrate a run/list. Returns the process exit code:
/// `0` all passed, `1` at least one failed, `2` harness error (no cases, bad
/// eval files). Splitting the exit code lets CI tell a real regression (`1`)
/// from a misconfigured path (`2`).
pub fn execute(
    dir: PathBuf,
    bin_dir: Option<PathBuf>,
    root: Option<PathBuf>,
    json_out: bool,
    list_only: bool,
) -> i32 {
    let root = root.unwrap_or_else(|| PathBuf::from("."));
    let eval_dir = if dir.is_absolute() {
        dir
    } else {
        root.join(&dir)
    };

    let discovered = match discover(&eval_dir) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("evalkit: {e:#}");
            return 2;
        }
    };
    let total: usize = discovered.iter().map(|(_, c)| c.len()).sum();
    if total == 0 {
        eprintln!(
            "evalkit: no cases found under {} (looked for *.jsonl)",
            eval_dir.display()
        );
        return 2;
    }

    if list_only {
        for (path, cases) in &discovered {
            for c in cases {
                println!("{}\t{}", path.display(), c.label());
            }
        }
        return 0;
    }

    let cfg = RunCfg { root, bin_dir };
    let mut outcomes = Vec::new();
    for (_, cases) in &discovered {
        for c in cases {
            outcomes.push(run_case(c, &cfg));
        }
    }

    let failed = outcomes.iter().filter(|o| !o.passed()).count();
    if json_out {
        report_json(&outcomes, failed);
    } else {
        report_human(&outcomes, failed);
    }
    if failed == 0 {
        0
    } else {
        1
    }
}

fn report_human(outcomes: &[Outcome], failed: usize) {
    for o in outcomes {
        if o.passed() {
            println!("  ok   {}", o.label);
        } else {
            println!("  FAIL {}", o.label);
            for f in &o.failures {
                println!("         - {f}");
            }
        }
    }
    let total = outcomes.len();
    println!(
        "\nevalkit: {}/{} passed, {} failed",
        total - failed,
        total,
        failed
    );
}

fn report_json(outcomes: &[Outcome], failed: usize) {
    let cases: Vec<_> = outcomes
        .iter()
        .map(|o| json!({"case": o.label, "pass": o.passed(), "failures": o.failures}))
        .collect();
    let total = outcomes.len();
    println!(
        "{}",
        json!({"total": total, "passed": total - failed, "failed": failed, "cases": cases})
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::case::Assert;

    fn assert_of(contains: &[&str], not_contains: &[&str]) -> Assert {
        Assert {
            contains: contains.iter().map(|s| s.to_string()).collect(),
            not_contains: not_contains.iter().map(|s| s.to_string()).collect(),
            ..Assert::default()
        }
    }

    #[test]
    fn contains_and_not_contains() {
        let a = assert_of(&["hello"], &["goodbye"]);
        let mut f = Vec::new();
        check_assert(&a, "hello world", &mut f);
        assert!(f.is_empty(), "{f:?}");

        let mut f = Vec::new();
        check_assert(&a, "goodbye world", &mut f);
        // missing "hello" AND forbidden "goodbye" present => two failures.
        assert_eq!(f.len(), 2, "{f:?}");
    }

    #[test]
    fn regex_match_and_forbidden() {
        let a = Assert {
            regex: vec![r"v\d+\.\d+".to_string()],
            not_regex: vec![r"ERROR".to_string()],
            ..Assert::default()
        };
        let mut f = Vec::new();
        check_assert(&a, "release v1.2 ok", &mut f);
        assert!(f.is_empty(), "{f:?}");

        let mut f = Vec::new();
        check_assert(&a, "no version, ERROR here", &mut f);
        assert_eq!(f.len(), 2, "{f:?}");
    }

    #[test]
    fn invalid_regex_is_a_failure_not_a_panic() {
        let a = Assert {
            regex: vec!["(".to_string()],
            ..Assert::default()
        };
        let mut f = Vec::new();
        check_assert(&a, "anything", &mut f);
        assert_eq!(f.len(), 1);
        assert!(f[0].contains("invalid regex"), "{:?}", f);
    }

    #[test]
    fn exit_mismatch_reported() {
        let a = Assert {
            exit: Some(0),
            ..Assert::default()
        };
        let mut f = Vec::new();
        check_exit(
            &a,
            &Subject {
                text: String::new(),
                exit: Some(1),
            },
            &mut f,
        );
        assert_eq!(f.len(), 1);
        assert!(f[0].contains("expected 0, got 1"), "{:?}", f);
    }

    #[test]
    fn exit_on_file_subject_is_a_failure() {
        let a = Assert {
            exit: Some(0),
            ..Assert::default()
        };
        let mut f = Vec::new();
        check_exit(
            &a,
            &Subject {
                text: String::new(),
                exit: None,
            },
            &mut f,
        );
        assert_eq!(f.len(), 1);
    }
}
