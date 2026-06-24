//! Run-state persistence and the completion gate.
//!
//! A run records each task's status and (once assigned) its worktree/branch.
//! State lives at `<state_dir>/<project-key>/<run-id>.json`. The gate is the
//! deterministic "are we actually done?" check the skill calls before declaring
//! success: every task verified, and no worktree left dirty or unremoved.

use crate::config::Config;
use crate::store::{project_key, repo_root};
use crate::worktree;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pending,
    Running,
    Done,
    Failed,
    Verified,
}

impl std::str::FromStr for Status {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "pending" => Status::Pending,
            "running" => Status::Running,
            "done" => Status::Done,
            "failed" => Status::Failed,
            "verified" => Status::Verified,
            other => bail!("unknown status '{other}' (pending|running|done|failed|verified)"),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub id: String,
    pub status: Status,
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub run_id: String,
    #[serde(default)]
    pub goal: String,
    pub tasks: Vec<TaskState>,
}

fn project_dir(cfg: &Config, cwd: &Path) -> PathBuf {
    let root = repo_root(cwd);
    cfg.state_dir.join(project_key(&root))
}

fn run_path(cfg: &Config, cwd: &Path, run_id: &str) -> PathBuf {
    project_dir(cfg, cwd).join(format!("{run_id}.json"))
}

impl RunState {
    pub fn load(cfg: &Config, cwd: &Path, run_id: &str) -> Result<Self> {
        let path = run_path(cfg, cwd, run_id);
        let txt = std::fs::read_to_string(&path)
            .with_context(|| format!("no run '{run_id}' at {}", path.display()))?;
        let state = serde_json::from_str(&txt).context("corrupt run-state JSON")?;
        Ok(state)
    }

    pub fn save(&self, cfg: &Config, cwd: &Path) -> Result<PathBuf> {
        let dir = project_dir(cfg, cwd);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creating state dir {}", dir.display()))?;
        let path = dir.join(format!("{}.json", self.run_id));
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
        Ok(path)
    }

    pub fn counts(&self) -> (usize, usize) {
        let done = self
            .tasks
            .iter()
            .filter(|t| t.status == Status::Verified)
            .count();
        (done, self.tasks.len())
    }
}

/// Open runs (not every task verified) for this project. Used by the SessionStart
/// hook and `state list`.
pub fn open_runs(cfg: &Config, cwd: &Path) -> Vec<RunState> {
    all_runs(cfg, cwd)
        .into_iter()
        .filter(|rs| {
            let (done, total) = rs.counts();
            done < total
        })
        .collect()
}

/// All runs (complete and incomplete) for this project, sorted by run_id.
pub fn all_runs(cfg: &Config, cwd: &Path) -> Vec<RunState> {
    let dir = project_dir(cfg, cwd);
    let mut runs = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            // Skip decomposition sidecars (run-id.decomposition.json).
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if fname.ends_with(".decomposition.json") {
                continue;
            }
            if let Ok(txt) = std::fs::read_to_string(&path) {
                if let Ok(rs) = serde_json::from_str::<RunState>(&txt) {
                    runs.push(rs);
                }
            }
        }
    }
    runs.sort_by(|a, b| a.run_id.cmp(&b.run_id));
    runs
}

// ── Decomposition sidecar ──────────────────────────────────────────────────

pub fn decomposition_path(cfg: &Config, cwd: &Path, run_id: &str) -> PathBuf {
    project_dir(cfg, cwd).join(format!("{run_id}.decomposition.json"))
}

/// Persist the raw decomposition JSON alongside the run state.
pub fn save_decomposition(cfg: &Config, cwd: &Path, run_id: &str, json: &str) -> Result<()> {
    let path = decomposition_path(cfg, cwd, run_id);
    std::fs::write(&path, json)
        .with_context(|| format!("writing decomposition to {}", path.display()))
}

/// Load the raw decomposition JSON for an existing run. Fails if not found.
pub fn load_decomposition(cfg: &Config, cwd: &Path, run_id: &str) -> Result<String> {
    let path = decomposition_path(cfg, cwd, run_id);
    std::fs::read_to_string(&path)
        .with_context(|| format!("no decomposition for run '{run_id}' at {}", path.display()))
}

// ── Stats ──────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct RunStats {
    pub run_id: String,
    pub goal: String,
    pub verified: usize,
    pub total: usize,
    pub is_complete: bool,
    pub status_counts: std::collections::HashMap<String, usize>,
}

pub fn compute_stats(cfg: &Config, cwd: &Path) -> Vec<RunStats> {
    all_runs(cfg, cwd)
        .into_iter()
        .map(|rs| {
            let mut counts = std::collections::HashMap::new();
            for t in &rs.tasks {
                let key = format!("{:?}", t.status).to_lowercase();
                *counts.entry(key).or_insert(0usize) += 1;
            }
            let (verified, total) = rs.counts();
            let is_complete = verified == total && total > 0;
            RunStats {
                run_id: rs.run_id,
                goal: rs.goal,
                verified,
                total,
                is_complete,
                status_counts: counts,
            }
        })
        .collect()
}

/// Reasons the run is NOT complete (empty = gate passes).
pub fn gate_reasons(cfg: &Config, cwd: &Path, run: &RunState) -> Vec<String> {
    let mut reasons = Vec::new();
    let repo = repo_root(cwd);

    for t in &run.tasks {
        if t.status != Status::Verified {
            reasons.push(format!("task '{}' is {:?}, not verified", t.id, t.status));
        }
        // A finished task must not leave its worktree behind, dirty or not.
        if let Some(wt) = &t.worktree {
            let p = PathBuf::from(wt);
            if p.exists() {
                match worktree::is_dirty(&p) {
                    Ok(true) => reasons
                        .push(format!("worktree for '{}' has uncommitted changes ({wt})", t.id)),
                    Ok(false) => {
                        reasons.push(format!("worktree for '{}' still exists ({wt})", t.id))
                    }
                    Err(_) => reasons.push(format!("worktree for '{}' unreadable ({wt})", t.id)),
                }
            }
        }
    }

    // Any orphan worktree under the base is also a leak.
    if let Ok(orphans) = worktree::orphans(&repo, &cfg.worktree_base) {
        for o in orphans {
            reasons.push(format!("orphan worktree on disk: {}", o.display()));
        }
    }

    reasons
}

/// Run the project's test suite (from the repo root) and propagate its result.
///
/// The command runs at the repo top-level, not the raw cwd, so invoking
/// `condukt state test` from a subdirectory still tests the whole project and
/// auto-detection sees the project's manifest. The command is handed to `sh -c`
/// so quoted args, pipes, and env vars in a configured `test_command` work as
/// the user expects (`pytest -k "foo bar"`).
pub fn run_tests(cfg: &Config, cwd: &Path, _rs: &RunState) -> Result<()> {
    let root = repo_root(cwd);
    let cmd_str = cfg
        .test_command
        .clone()
        .unwrap_or_else(|| auto_detect_test_command(&root));
    if cmd_str.trim().is_empty() {
        bail!("empty test command");
    }
    eprintln!("condukt: running tests in {}: {cmd_str}", root.display());
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd_str)
        .current_dir(&root)
        .status()
        .with_context(|| format!("failed to run '{cmd_str}'"))?;
    if status.success() {
        eprintln!("condukt: tests passed");
        Ok(())
    } else {
        bail!("tests failed (exit {})", status.code().unwrap_or(-1))
    }
}

fn auto_detect_test_command(cwd: &Path) -> String {
    if cwd.join("Cargo.toml").exists() {
        return "cargo test".to_string();
    }
    if cwd.join("package.json").exists() {
        return "npm test".to_string();
    }
    if cwd.join("pyproject.toml").exists() || cwd.join("setup.py").exists() {
        return "pytest".to_string();
    }
    "cargo test".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_roundtrip() {
        for s in ["pending", "running", "done", "failed", "verified"] {
            let st: Status = s.parse().unwrap();
            let json = serde_json::to_string(&st).unwrap();
            assert_eq!(json, format!("\"{s}\""));
        }
        assert!("bogus".parse::<Status>().is_err());
    }

    #[test]
    fn counts_only_verified() {
        let rs = RunState {
            run_id: "r1".into(),
            goal: "g".into(),
            tasks: vec![
                TaskState {
                    id: "a".into(),
                    status: Status::Verified,
                    worktree: None,
                    branch: None,
                },
                TaskState {
                    id: "b".into(),
                    status: Status::Done,
                    worktree: None,
                    branch: None,
                },
            ],
        };
        assert_eq!(rs.counts(), (1, 2));
    }

    fn make_tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("condukt-test-{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn auto_detect_cargo() {
        let dir = make_tmp_dir("auto-cargo");
        std::fs::write(dir.join("Cargo.toml"), "[package]").unwrap();
        assert_eq!(auto_detect_test_command(&dir), "cargo test");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn auto_detect_npm() {
        let dir = make_tmp_dir("auto-npm");
        std::fs::write(dir.join("package.json"), "{}").unwrap();
        assert_eq!(auto_detect_test_command(&dir), "npm test");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn auto_detect_pytest() {
        let dir = make_tmp_dir("auto-pytest");
        std::fs::write(dir.join("pyproject.toml"), "").unwrap();
        assert_eq!(auto_detect_test_command(&dir), "pytest");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn auto_detect_fallback() {
        let dir = make_tmp_dir("auto-fallback");
        // empty dir — no recognizable project files
        assert_eq!(auto_detect_test_command(&dir), "cargo test");
        std::fs::remove_dir_all(&dir).ok();
    }
}
