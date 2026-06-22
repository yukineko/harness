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
    let dir = project_dir(cfg, cwd);
    let mut runs = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(txt) = std::fs::read_to_string(&path) {
                if let Ok(rs) = serde_json::from_str::<RunState>(&txt) {
                    let (done, total) = rs.counts();
                    if done < total {
                        runs.push(rs);
                    }
                }
            }
        }
    }
    runs.sort_by(|a, b| a.run_id.cmp(&b.run_id));
    runs
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
}
