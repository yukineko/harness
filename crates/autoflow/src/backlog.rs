use std::path::{Path, PathBuf};

use harness_core::config::home;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct BacklogItem {
    pub id: String,
    pub text: String,
    pub status: String,
}

/// Find open backlog items for the repo containing `cwd`.
/// Returns empty vec if session-insights binary is not found or no open items.
pub fn find_open(cwd: &Path) -> Vec<BacklogItem> {
    let binary = match find_si_binary() {
        Some(b) => b,
        None => return vec![],
    };

    let project = repo_basename(cwd);

    let output = std::process::Command::new(&binary)
        .args(["backlog", "list", "--project", &project, "--json"])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    let items: Vec<BacklogItem> = serde_json::from_slice(&output.stdout).unwrap_or_default();

    items.into_iter().filter(|i| i.status == "open").collect()
}

/// Locate the session-insights binary: PATH first, then plugin cache.
fn find_si_binary() -> Option<PathBuf> {
    if std::process::Command::new("session-insights")
        .arg("--version")
        .output()
        .is_ok()
    {
        return Some(PathBuf::from("session-insights"));
    }

    // ~/.claude/plugins/cache/yukineko/session-insights/<version>/bin/session-insights
    let base = home()
        .join(".claude")
        .join("plugins")
        .join("cache")
        .join("yukineko")
        .join("session-insights");

    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&base)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path().join("bin").join("session-insights"))
        .filter(|p| p.exists())
        .collect();

    candidates.sort();
    candidates.pop()
}

fn repo_basename(cwd: &Path) -> String {
    let root = repo_root(cwd);
    root.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
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
