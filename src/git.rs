//! Determine which files have changed so checks can be scoped (don't run
//! `cargo test` when only markdown changed). Pure subprocess calls to `git`.

use std::path::Path;
use std::process::Command;

/// Changed paths relative to the repo root: tracked changes vs HEAD plus
/// untracked-but-not-ignored files. `None` means "not a git repo / git
/// unavailable" — callers then treat every check as applicable.
pub fn changed_files(root: &Path) -> Option<Vec<String>> {
    if !is_git_repo(root) {
        return None;
    }
    let mut out = Vec::new();
    // tracked, unstaged changes
    collect(root, &["diff", "--name-only"], &mut out);
    // staged changes — also the only signal in a repo with no commits yet, where
    // `diff HEAD` errors out (no HEAD to diff against).
    collect(root, &["diff", "--cached", "--name-only"], &mut out);
    // untracked (respecting .gitignore)
    collect(
        root,
        &["ls-files", "--others", "--exclude-standard"],
        &mut out,
    );
    out.sort();
    out.dedup();
    Some(out)
}

fn is_git_repo(root: &Path) -> bool {
    Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn collect(root: &Path, args: &[&str], out: &mut Vec<String>) {
    if let Ok(o) = Command::new("git").current_dir(root).args(args).output() {
        if o.status.success() {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let line = line.trim();
                if !line.is_empty() {
                    out.push(line.to_string());
                }
            }
        }
    }
}
