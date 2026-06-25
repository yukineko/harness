use std::path::{Path, PathBuf};

use harness_core::config::home;
use serde::{Deserialize, Serialize};

/// 2 hours in seconds. Running tasks older than this are considered interrupted.
const STUCK_SECS: i64 = 7200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RunState {
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub tasks: Vec<TaskState>,
}

/// Find pending/failed tasks for the repo containing `cwd`.
///
/// Before returning, reverts any `running` task whose `updated_at` is older
/// than 2 hours — those were almost certainly interrupted mid-session.
pub fn find_pending(cwd: &Path) -> Vec<TaskState> {
    let (path, mut run) = match load_latest(cwd) {
        Some(x) => x,
        None => return vec![],
    };

    let now = now_secs();
    let mut modified = false;
    for task in &mut run.tasks {
        if task.status == "running" {
            let age = task.updated_at.map(|t| now - t).unwrap_or(i64::MAX);
            if age > STUCK_SECS {
                task.status = "pending".to_string();
                task.updated_at = None;
                modified = true;
            }
        }
    }
    if modified {
        save_run(&path, &run);
    }

    run.tasks
        .into_iter()
        .filter(|t| matches!(t.status.as_str(), "pending" | "failed"))
        .collect()
}

/// Mark the given task IDs as `running` (with current timestamp) in the most
/// recent condukt run for the repo containing `cwd`.
pub fn mark_running(cwd: &Path, task_ids: &[&str]) {
    let (path, mut run) = match load_latest(cwd) {
        Some(x) => x,
        None => return,
    };
    let now = now_secs();
    let mut modified = false;
    for task in &mut run.tasks {
        if task_ids.contains(&task.id.as_str())
            && matches!(task.status.as_str(), "pending" | "failed")
        {
            task.status = "running".to_string();
            task.updated_at = Some(now);
            modified = true;
        }
    }
    if modified {
        save_run(&path, &run);
    }
}

fn load_latest(cwd: &Path) -> Option<(PathBuf, RunState)> {
    let root = repo_root(cwd);
    let key = project_key(&root);
    let project_dir = home().join(".condukt").join("state").join(&key);
    let path = latest_run_file(&project_dir)?;
    let run = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<RunState>(&t).ok())?;
    Some((path, run))
}

fn save_run(path: &Path, run: &RunState) {
    if let Ok(text) = serde_json::to_string_pretty(run) {
        let _ = std::fs::write(path, text);
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn latest_run_file(project_dir: &Path) -> Option<PathBuf> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(project_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("run-") && n.ends_with(".json"))
                .unwrap_or(false)
        })
        .collect();
    entries.sort();
    entries.pop()
}

fn repo_root(cwd: &Path) -> PathBuf {
    let mut cur = cwd.to_path_buf();
    loop {
        if cur.join(".git").exists() {
            return cur;
        }
        if !cur.pop() {
            break;
        }
    }
    cwd.to_path_buf()
}

fn project_key(root: &Path) -> String {
    let canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let full = canon.to_string_lossy();
    let base = canon
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "root".into());
    let sani: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("{}-{:08x}", sani, fnv1a32(&full))
}

fn fnv1a32(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}
