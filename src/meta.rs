//! Wiki freshness tracking. After a wiki is (re)generated, `stamp` records the
//! current git commit in `.deepwiki/_meta.toml`; `status` compares it to HEAD
//! and lists the source files that changed since, so the wiki can be refreshed
//! only when the code it documents actually moved.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use serde::{Deserialize, Serialize};

pub const WIKI_DIR: &str = ".deepwiki";
const META_FILE: &str = "_meta.toml";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub sha: String,
    #[serde(default)]
    pub built_at: String,
    #[serde(default)]
    pub pages: Vec<String>,
}

pub fn wiki_dir(root: &Path) -> PathBuf {
    root.join(WIKI_DIR)
}

pub fn meta_path(root: &Path) -> PathBuf {
    wiki_dir(root).join(META_FILE)
}

pub fn load(root: &Path) -> Option<Meta> {
    let text = std::fs::read_to_string(meta_path(root)).ok()?;
    toml::from_str(&text).ok()
}

/// Current `git rev-parse HEAD` for `root`, or None outside a repo.
pub fn head_sha(root: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["-C", &root.to_string_lossy(), "rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Source files changed between `from` SHA and HEAD (best-effort).
pub fn changed_since(root: &Path, from: &str) -> Vec<String> {
    let Ok(out) = Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "diff",
            "--name-only",
            &format!("{from}..HEAD"),
        ])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect()
}

pub fn stamp(root: &Path, pages: Vec<String>) -> Result<()> {
    std::fs::create_dir_all(wiki_dir(root))?;
    let meta = Meta {
        sha: head_sha(root).unwrap_or_default(),
        built_at: chrono::Local::now().to_rfc3339(),
        pages,
    };
    let text = toml::to_string_pretty(&meta)?;
    std::fs::write(meta_path(root), text)?;
    Ok(())
}
