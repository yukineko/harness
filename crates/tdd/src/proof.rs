//! Test-first proof: make "the test was written before the implementation"
//! a *verifiable* artifact instead of a claim.
//!
//!   `tdd red`   runs the tests and REQUIRES them to fail (≥1 red). It records
//!               `<proof_dir>/<task>.red.json`. If they already pass, that's not
//!               test-first — it errors.
//!   `tdd green` REQUIRES a prior RED proof, runs the tests, and REQUIRES them to
//!               pass. It records `<proof_dir>/<task>.green.json`.
//!   `tdd verify` succeeds iff both proofs exist (RED then GREEN happened).

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use serde_json::json;

use crate::config::Config;
use crate::runner;

fn safe(task: &str) -> String {
    task.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

pub fn proof_dir(root: &Path, cfg: &Config) -> PathBuf {
    root.join(&cfg.proof_dir)
}

pub fn artifact_path(root: &Path, cfg: &Config, task: &str, kind: &str) -> PathBuf {
    proof_dir(root, cfg).join(format!("{}.{kind}.json", safe(task)))
}

/// Decide whether a RED run is acceptable: the tests MUST have failed.
fn judge_red(passed: bool) -> Result<()> {
    if passed {
        bail!(
            "tests passed on `tdd red` — that is not test-first. Write a test that FAILS \
             against the not-yet-written behaviour first, then run `tdd red` again."
        );
    }
    Ok(())
}

/// Decide whether a GREEN run is acceptable: a RED proof must exist and the
/// tests MUST now pass.
fn judge_green(passed: bool, has_red: bool) -> Result<()> {
    if !has_red {
        bail!("no RED proof found — run `tdd red --task <id>` before implementing.");
    }
    if !passed {
        bail!("tests still failing — keep implementing until they pass, then run `tdd green` again.");
    }
    Ok(())
}

fn write_artifact(path: &Path, value: &serde_json::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}

fn resolve_cmd<'a>(cmd: &'a Option<String>, cfg: &'a Config) -> &'a str {
    match cmd {
        Some(c) if !c.trim().is_empty() => c,
        _ => &cfg.test_cmd,
    }
}

/// `tdd red`: run the tests, require failure, record the RED proof.
pub fn red(root: &Path, cfg: &Config, task: &str, cmd: &Option<String>) -> Result<()> {
    let cmdline = resolve_cmd(cmd, cfg);
    let tmp = cfg.state_dir.join("tmp");
    let out = runner::run_cmd(
        cmdline,
        root,
        cfg.default_timeout_secs,
        cfg.output_tail_lines,
        &tmp,
    );
    judge_red(out.passed)?;
    let path = artifact_path(root, cfg, task, "red");
    write_artifact(
        &path,
        &json!({
            "task": task,
            "phase": "red",
            "cmd": cmdline,
            "passed": out.passed,
            "exit_code": out.exit_code,
            "ts": chrono::Local::now().to_rfc3339(),
            "output_tail": out.output_tail,
        }),
    )?;
    println!(
        "🔴 RED recorded for `{task}` — tests fail as expected ({}).\n   {}",
        out.status_str(),
        path.display()
    );
    Ok(())
}

/// `tdd green`: require a RED proof, run the tests, require success, record GREEN.
pub fn green(root: &Path, cfg: &Config, task: &str, cmd: &Option<String>) -> Result<()> {
    let red_path = artifact_path(root, cfg, task, "red");
    let has_red = red_path.exists();
    let cmdline = resolve_cmd(cmd, cfg);
    let tmp = cfg.state_dir.join("tmp");
    let out = runner::run_cmd(
        cmdline,
        root,
        cfg.default_timeout_secs,
        cfg.output_tail_lines,
        &tmp,
    );
    judge_green(out.passed, has_red)?;
    let path = artifact_path(root, cfg, task, "green");
    write_artifact(
        &path,
        &json!({
            "task": task,
            "phase": "green",
            "cmd": cmdline,
            "passed": out.passed,
            "exit_code": out.exit_code,
            "ts": chrono::Local::now().to_rfc3339(),
            "red_proof": red_path.display().to_string(),
        }),
    )?;
    println!(
        "🟢 GREEN recorded for `{task}` — tests pass after RED.\n   {}",
        path.display()
    );
    Ok(())
}

/// `tdd verify`: true iff both RED and GREEN proofs exist for the task.
pub fn verify(root: &Path, cfg: &Config, task: &str) -> bool {
    artifact_path(root, cfg, task, "red").exists()
        && artifact_path(root, cfg, task, "green").exists()
}

impl runner::Outcome {
    fn status_str(&self) -> String {
        if self.timed_out {
            "timed out".to_string()
        } else if let Some(e) = &self.spawn_error {
            e.clone()
        } else {
            match self.exit_code {
                Some(c) => format!("exit {c}"),
                None => "killed".to_string(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn red_requires_failure() {
        assert!(judge_red(false).is_ok()); // failed → good
        assert!(judge_red(true).is_err()); // passed → not test-first
    }

    #[test]
    fn green_requires_red_then_pass() {
        assert!(judge_green(true, true).is_ok());
        assert!(judge_green(true, false).is_err()); // no RED proof
        assert!(judge_green(false, true).is_err()); // still failing
    }

    #[test]
    fn verify_needs_both_artifacts() {
        let base = std::env::temp_dir().join(format!("tdd-proof-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let cfg = Config {
            proof_dir: ".tdd".to_string(),
            ..Config::default()
        };
        std::fs::create_dir_all(&base).unwrap();
        assert!(!verify(&base, &cfg, "t1"));
        write_artifact(&artifact_path(&base, &cfg, "t1", "red"), &json!({"x":1})).unwrap();
        assert!(!verify(&base, &cfg, "t1"));
        write_artifact(&artifact_path(&base, &cfg, "t1", "green"), &json!({"x":1})).unwrap();
        assert!(verify(&base, &cfg, "t1"));
        let _ = std::fs::remove_dir_all(&base);
    }
}
