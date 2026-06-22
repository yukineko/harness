//! Deterministic git-worktree lifecycle. Wraps `git worktree` so the skill
//! never hand-rolls these commands (and so the invariants — path outside the
//! repo, one branch per dir, clean removal — are enforced in code, not prose).

use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Run `git` with args in `dir`, returning trimmed stdout. Errors include stderr.
pub fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .with_context(|| format!("failed to spawn git {:?}", args))?;
    if !out.status.success() {
        bail!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Repo root for `cwd` per git itself.
pub fn toplevel(cwd: &Path) -> Result<PathBuf> {
    let s = git(cwd, &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(s))
}

/// Is `branch` already checked out in some worktree of this repo?
fn branch_checked_out(repo: &Path, branch: &str) -> Result<bool> {
    let listing = git(repo, &["worktree", "list", "--porcelain"])?;
    let needle = format!("branch refs/heads/{branch}");
    Ok(listing.lines().any(|l| l.trim() == needle))
}

/// Create a worktree at `<worktree_base>/<topic>` on a new `branch`.
/// Enforces: path is outside the repo, and the branch isn't already checked out.
pub fn create(repo: &Path, worktree_base: &Path, topic: &str, branch: &str) -> Result<PathBuf> {
    let repo_canon = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
    let path = worktree_base.join(topic);

    if path.starts_with(&repo_canon) {
        bail!(
            "refusing to create worktree inside the repo ({}); set worktree_base outside it",
            path.display()
        );
    }
    if path.exists() {
        bail!("worktree path already exists: {}", path.display());
    }
    if branch_checked_out(repo, branch)? {
        bail!("branch '{branch}' is already checked out in another worktree (one dir = one branch)");
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let path_str = path.to_string_lossy().to_string();
    git(repo, &["worktree", "add", &path_str, "-b", branch])?;
    Ok(path)
}

/// Merge `branch` into the configured default branch. Verifies the repo is
/// actually on the default branch first (verify -> act).
pub fn merge(repo: &Path, branch: &str, default_branch: &str) -> Result<()> {
    git(repo, &["checkout", default_branch])
        .with_context(|| format!("could not checkout {default_branch} before merge"))?;
    let current = git(repo, &["branch", "--show-current"])?;
    if current != default_branch {
        bail!("expected to be on '{default_branch}' but on '{current}'; aborting merge");
    }
    git(repo, &["merge", "--no-edit", branch])
        .with_context(|| format!("merge of {branch} into {default_branch} failed"))?;
    Ok(())
}

/// Remove the worktree at `path` and delete its `branch` (best-effort on branch).
pub fn remove(repo: &Path, path: &Path, branch: Option<&str>) -> Result<()> {
    let path_str = path.to_string_lossy().to_string();
    git(repo, &["worktree", "remove", &path_str])
        .with_context(|| format!("could not remove worktree {}", path.display()))?;
    if let Some(b) = branch {
        // -d (safe delete) only; leave unmerged branches for the user.
        let _ = git(repo, &["branch", "-d", b]);
    }
    Ok(())
}

/// (path, branch) pairs for every registered worktree except the primary.
pub fn list(repo: &Path) -> Result<Vec<(PathBuf, Option<String>)>> {
    let listing = git(repo, &["worktree", "list", "--porcelain"])?;
    let primary = toplevel(repo)?.canonicalize().ok();
    let mut out = Vec::new();
    let mut cur_path: Option<PathBuf> = None;
    let mut cur_branch: Option<String> = None;
    for line in listing.lines().chain(std::iter::once("")) {
        if let Some(p) = line.strip_prefix("worktree ") {
            // flush previous
            if let Some(path) = cur_path.take() {
                let is_primary = path.canonicalize().ok() == primary;
                if !is_primary {
                    out.push((path, cur_branch.take()));
                }
            }
            cur_branch = None;
            cur_path = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            cur_branch = Some(b.to_string());
        } else if line.is_empty() {
            if let Some(path) = cur_path.take() {
                let is_primary = path.canonicalize().ok() == primary;
                if !is_primary {
                    out.push((path, cur_branch.take()));
                }
            }
        }
    }
    Ok(out)
}

/// Worktree dirs physically under `worktree_base` that git no longer tracks.
pub fn orphans(repo: &Path, worktree_base: &Path) -> Result<Vec<PathBuf>> {
    if !worktree_base.exists() {
        return Ok(Vec::new());
    }
    let registered: Vec<PathBuf> = list(repo)?
        .into_iter()
        .filter_map(|(p, _)| p.canonicalize().ok())
        .collect();
    let mut orphans = Vec::new();
    for entry in std::fs::read_dir(worktree_base)
        .with_context(|| format!("reading {}", worktree_base.display()))?
    {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !registered.contains(&canon) {
            orphans.push(path);
        }
    }
    Ok(orphans)
}

/// Does a worktree have uncommitted changes (tracked or untracked)?
pub fn is_dirty(path: &Path) -> Result<bool> {
    let status = git(path, &["status", "--porcelain"])
        .map_err(|e| anyhow!("status check failed for {}: {e}", path.display()))?;
    Ok(!status.trim().is_empty())
}
