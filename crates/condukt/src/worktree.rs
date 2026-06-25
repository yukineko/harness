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

/// Run a git command without bailing on non-zero exit; return (success, stdout, stderr).
fn git_try(dir: &Path, args: &[&str]) -> Result<(bool, String, String)> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .with_context(|| format!("failed to spawn git {:?}", args))?;
    Ok((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
        String::from_utf8_lossy(&out.stderr).trim().to_string(),
    ))
}

/// Merge `branch` into the configured default branch. Verifies the repo is
/// actually on the default branch first (verify -> act).
///
/// Pre-flight: attempts `git merge --no-commit --no-ff` to detect conflicts
/// before performing the real merge. If conflicts are detected, aborts the
/// trial merge and returns an error without touching the branch history.
pub fn merge(repo: &Path, branch: &str, default_branch: &str) -> Result<()> {
    git(repo, &["checkout", default_branch])
        .with_context(|| format!("could not checkout {default_branch} before merge"))?;
    let current = git(repo, &["branch", "--show-current"])?;
    if current != default_branch {
        bail!("expected to be on '{default_branch}' but on '{current}'; aborting merge");
    }

    // ── Pre-flight: trial merge (no-commit) to detect conflicts ──────────────
    // `git merge --no-commit --no-ff` either succeeds with a staged merge or
    // fails immediately when conflicts exist. In both cases we abort and then
    // re-run the real merge only when there are no conflicts.
    let (trial_ok, _, trial_stderr) =
        git_try(repo, &["merge", "--no-commit", "--no-ff", branch])?;

    if !trial_ok {
        // Trial merge reported conflicts. Abort to restore clean state.
        let _ = git_try(repo, &["merge", "--abort"]);
        bail!(
            "merge of '{branch}' into '{default_branch}' has conflicts (pre-flight); \
             aborting without modifying history.\n{trial_stderr}"
        );
    }

    // Even a "successful" --no-commit merge may leave CONFLICT markers when
    // git decides to apply both sides with markers rather than refusing. Use
    // `git ls-files --unmerged` to catch those cases.
    let unmerged = git(repo, &["ls-files", "--unmerged"])?;
    if !unmerged.trim().is_empty() {
        let _ = git_try(repo, &["merge", "--abort"]);
        bail!(
            "merge of '{branch}' into '{default_branch}' has unresolved conflicts (pre-flight); \
             aborting without modifying history."
        );
    }

    // No conflicts found in trial — abort the staged trial and do the real merge.
    git_try(repo, &["merge", "--abort"])
        .with_context(|| "could not abort trial merge before real merge")?;

    git(repo, &["merge", "--no-edit", branch])
        .with_context(|| format!("merge of {branch} into {default_branch} failed"))?;
    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod worktree_remove_tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Initialise a bare-minimum git repo with an initial commit on `main`.
    fn init_repo() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().expect("tempdir");
        let repo = tmp.path().to_path_buf();

        git(&repo, &["init", "-b", "main"]).unwrap();
        git(&repo, &["config", "user.email", "test@example.com"]).unwrap();
        git(&repo, &["config", "user.name", "Test"]).unwrap();

        // Initial commit on main
        let f = repo.join("base.txt");
        fs::write(&f, "base\n").unwrap();
        git(&repo, &["add", "."]).unwrap();
        git(&repo, &["commit", "-m", "init"]).unwrap();

        (tmp, repo)
    }

    /// Create a branch from HEAD, write `content` to `file`, commit and return.
    fn make_branch(repo: &Path, branch: &str, file: &str, content: &str) {
        git(repo, &["checkout", "-b", branch]).unwrap();
        fs::write(repo.join(file), content).unwrap();
        git(repo, &["add", "."]).unwrap();
        git(
            repo,
            &["commit", "-m", &format!("add {file} on {branch}")],
        )
        .unwrap();
        git(repo, &["checkout", "main"]).unwrap();
    }

    #[test]
    fn worktree_merge_no_conflict_succeeds() {
        let (_tmp, repo) = init_repo();
        make_branch(&repo, "feat", "feat.txt", "feature content\n");

        merge(&repo, "feat", "main").expect("clean merge should succeed");

        // The file should now exist on main
        assert!(repo.join("feat.txt").exists());
    }

    #[test]
    fn worktree_merge_conflict_returns_error() {
        let (_tmp, repo) = init_repo();

        // Both branches modify the same file at the same line → guaranteed conflict
        let conflict_file = "shared.txt";

        // Write a shared base first on main
        fs::write(repo.join(conflict_file), "line1\nline2\nline3\n").unwrap();
        git(&repo, &["add", "."]).unwrap();
        git(&repo, &["commit", "-m", "add shared file"]).unwrap();

        // Branch: modify line2 to "branch version"
        git(&repo, &["checkout", "-b", "conflict-branch"]).unwrap();
        fs::write(
            repo.join(conflict_file),
            "line1\nbranch version\nline3\n",
        )
        .unwrap();
        git(&repo, &["add", "."]).unwrap();
        git(&repo, &["commit", "-m", "branch edit"]).unwrap();

        // Main: modify line2 differently → creates a real conflict
        git(&repo, &["checkout", "main"]).unwrap();
        fs::write(
            repo.join(conflict_file),
            "line1\nmain version\nline3\n",
        )
        .unwrap();
        git(&repo, &["add", "."]).unwrap();
        git(&repo, &["commit", "-m", "main edit"]).unwrap();

        let result = merge(&repo, "conflict-branch", "main");
        assert!(
            result.is_err(),
            "conflicting merge should return an error, but got Ok"
        );

        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(
            err_msg.contains("conflict") || err_msg.contains("unresolved"),
            "error message should mention conflicts, got: {err_msg}"
        );

        // The repo should be in a clean state (no in-progress merge)
        let merge_head = repo.join(".git").join("MERGE_HEAD");
        assert!(
            !merge_head.exists(),
            "MERGE_HEAD should not exist after aborted pre-flight"
        );
    }

    #[test]
    fn worktree_is_dirty_clean_repo() {
        let (_tmp, repo) = init_repo();
        assert!(!is_dirty(&repo).expect("is_dirty should not error on a clean repo"));
    }

    #[test]
    fn worktree_is_dirty_with_uncommitted_change() {
        let (_tmp, repo) = init_repo();
        fs::write(repo.join("new.txt"), "dirty\n").unwrap();
        assert!(is_dirty(&repo).expect("is_dirty should not error"));
    }
}

/// Remove the worktree at `path` and delete its `branch` (best-effort on branch).
///
/// If the branch cannot be deleted because it is not fully merged, a warning is
/// printed to stderr and the function still returns `Ok(())`. The caller is
/// responsible for acting on the warning (e.g. the CLI prints it again with
/// context).
///
/// Returns `Some(branch_name)` when the branch was NOT deleted (unmerged or
/// any other error), `None` when no branch was requested or deletion succeeded.
pub fn remove(repo: &Path, path: &Path, branch: Option<&str>) -> Result<Option<String>> {
    let path_str = path.to_string_lossy().to_string();
    git(repo, &["worktree", "remove", &path_str])
        .with_context(|| format!("could not remove worktree {}", path.display()))?;
    if let Some(b) = branch {
        match git(repo, &["branch", "-d", b]) {
            Ok(_) => {}
            Err(e) => {
                // The branch still exists — most likely it was not fully merged.
                // Warn on stderr and surface the branch name to the caller so it
                // can display a more actionable message.
                eprintln!(
                    "warning: branch '{}' was not deleted (not fully merged). \
                     Use `git branch -D {}` to force-delete, or merge it first.",
                    b, b
                );
                eprintln!("  (git said: {e})");
                return Ok(Some(b.to_string()));
            }
        }
    }
    Ok(None)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Initialise a bare-minimum git repo in `dir` (main branch, one commit).
    fn init_repo(dir: &Path) {
        git(dir, &["init", "-b", "main"]).unwrap();
        git(dir, &["config", "user.email", "test@example.com"]).unwrap();
        git(dir, &["config", "user.name", "Test"]).unwrap();
        // Commit something so HEAD is valid.
        let readme = dir.join("README.md");
        fs::write(&readme, "initial").unwrap();
        git(dir, &["add", "README.md"]).unwrap();
        git(dir, &["commit", "-m", "initial"]).unwrap();
    }

    /// remove() returns Ok(None) when the branch was merged and deletes cleanly.
    #[test]
    fn remove_returns_none_when_branch_merged() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        // Create a branch that we immediately merge back in — it is "merged".
        git(&repo, &["checkout", "-b", "feat/merged"]).unwrap();
        git(&repo, &["checkout", "main"]).unwrap();
        git(&repo, &["merge", "--no-edit", "feat/merged"]).unwrap();

        // Re-create the branch so we can try to remove it via add+remove worktree.
        // The branch has no unique commits so -d will succeed.
        // To avoid checking out the branch in a worktree we just test branch -d
        // directly via the expected code path: a branch that is fully merged.
        // We simulate by calling remove() with a non-existent path (after prune)
        // We need a real worktree for the `git worktree remove` to work, so:
        let wt_base = tmp.path().join("worktrees");
        fs::create_dir_all(&wt_base).unwrap();
        let wt_path = wt_base.join("merged-wt");
        // Create a new branch for the worktree (identical content to main).
        git(&repo, &["worktree", "add", wt_path.to_str().unwrap(), "-b", "feat/wt-merged"]).unwrap();
        // Merge it.
        git(&repo, &["merge", "--no-edit", "feat/wt-merged"]).unwrap();
        // Now remove: branch -d should succeed because it is merged.
        let result = remove(&repo, &wt_path, Some("feat/wt-merged")).unwrap();
        assert_eq!(result, None, "merged branch should be deleted: got {:?}", result);
    }

    /// remove() returns Ok(Some(branch)) when the branch is NOT merged.
    #[test]
    fn remove_returns_branch_name_when_unmerged() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let wt_base = tmp.path().join("worktrees");
        fs::create_dir_all(&wt_base).unwrap();
        let wt_path = wt_base.join("unmerged-wt");
        git(&repo, &["worktree", "add", wt_path.to_str().unwrap(), "-b", "feat/unmerged"]).unwrap();

        // Add a commit to the worktree so it diverges from main.
        let extra = wt_path.join("extra.txt");
        fs::write(&extra, "unique").unwrap();
        git(&wt_path, &["add", "extra.txt"]).unwrap();
        git(&wt_path, &["commit", "-m", "diverge"]).unwrap();

        // Now remove: -d should fail because feat/unmerged is not merged.
        let result = remove(&repo, &wt_path, Some("feat/unmerged")).unwrap();
        assert_eq!(
            result.as_deref(),
            Some("feat/unmerged"),
            "unmerged branch should be returned"
        );

        // The branch must still exist in the repo.
        let branches = git(&repo, &["branch"]).unwrap();
        assert!(
            branches.contains("feat/unmerged"),
            "unmerged branch should still exist after remove; got: {}",
            branches
        );
    }

    /// orphans() returns directories under worktree_base not tracked by git.
    #[test]
    fn orphans_detects_unregistered_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let wt_base = tmp.path().join("worktrees");
        fs::create_dir_all(&wt_base).unwrap();

        // A directory that git never knew about.
        let ghost = wt_base.join("ghost-dir");
        fs::create_dir_all(&ghost).unwrap();

        let found = orphans(&repo, &wt_base).unwrap();
        assert!(
            found.iter().any(|p| p.ends_with("ghost-dir")),
            "ghost-dir should be reported as orphan; got: {:?}",
            found
        );
    }

    /// orphans() does not list a legitimately registered worktree.
    #[test]
    fn orphans_excludes_registered_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let wt_base = tmp.path().join("worktrees");
        fs::create_dir_all(&wt_base).unwrap();
        let wt_path = wt_base.join("registered");
        git(&repo, &["worktree", "add", wt_path.to_str().unwrap(), "-b", "feat/reg"]).unwrap();

        let found = orphans(&repo, &wt_base).unwrap();
        assert!(
            !found.iter().any(|p| p.ends_with("registered")),
            "registered worktree should not be listed as orphan; got: {:?}",
            found
        );
    }
}
