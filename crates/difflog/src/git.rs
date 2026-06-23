//! Git helpers: find the session's start commit and generate a diff summary.

use std::path::Path;
use std::process::Command;

/// The first commit SHA of the current session (the HEAD at session start).
/// We persist this in a state file; see `state.rs`.
/// If unavailable, fall back to the parent of the current HEAD.
pub fn head_sha(root: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

/// `git diff --stat <base>..HEAD` — file-level change summary.
pub fn diff_stat(root: &Path, base: &str) -> String {
    let out = Command::new("git")
        .args(["diff", "--stat", &format!("{base}..HEAD")])
        .current_dir(root)
        .output();
    match out {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).into_owned()
        }
        _ => String::new(),
    }
}

/// `git diff --name-status <base>..HEAD` — A/M/D per file.
pub fn diff_name_status(root: &Path, base: &str) -> Vec<(char, String)> {
    let out = Command::new("git")
        .args(["diff", "--name-status", &format!("{base}..HEAD")])
        .current_dir(root)
        .output();
    let Ok(o) = out else { return Vec::new() };
    if !o.status.success() { return Vec::new(); }
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '\t');
            let status = parts.next()?.chars().next()?;
            let path = parts.next()?.to_string();
            Some((status, path))
        })
        .collect()
}

/// Bounded `git diff <base>..HEAD` body (excludes binary files).
pub fn diff_body(root: &Path, base: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    let out = Command::new("git")
        .args(["diff", "--diff-filter=ACMRT", &format!("{base}..HEAD")])
        .current_dir(root)
        .output();
    let Ok(o) = out else { return String::new() };
    if !o.status.success() { return String::new(); }
    let full = String::from_utf8_lossy(&o.stdout);
    if full.len() <= limit {
        full.into_owned()
    } else {
        format!("{}\n… (truncated at {} bytes)", &full[..limit], limit)
    }
}

/// One-line commit log from `base` to HEAD.
pub fn log_oneline(root: &Path, base: &str) -> String {
    let out = Command::new("git")
        .args(["log", "--oneline", &format!("{base}..HEAD")])
        .current_dir(root)
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => String::new(),
    }
}
