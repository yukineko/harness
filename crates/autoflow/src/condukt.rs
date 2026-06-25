use std::path::{Path, PathBuf};

use harness_core::config::home;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct TaskState {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Default, Deserialize)]
struct RunState {
    #[serde(default)]
    pub tasks: Vec<TaskState>,
}

/// Find pending/running/failed tasks from the most recent condukt run for the
/// repo containing `cwd`. Returns an empty vec when there is no condukt state
/// or when all tasks are verified/done.
pub fn find_pending(cwd: &Path) -> Vec<TaskState> {
    let root = repo_root(cwd);
    let key = project_key(&root);
    let project_dir = home()
        .join(".condukt")
        .join("state")
        .join(&key);

    let latest = latest_run_file(&project_dir);
    let run = latest
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<RunState>(&t).ok())
        .unwrap_or_default();

    run.tasks
        .into_iter()
        .filter(|t| matches!(t.status.as_str(), "pending" | "running" | "failed"))
        .collect()
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

/// Nearest ancestor containing `.git`; falls back to `cwd`.
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

/// Stable per-project key matching condukt's own implementation:
/// `<sanitized-basename>-<fnv1a32-hex-of-canonical-path>`.
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
