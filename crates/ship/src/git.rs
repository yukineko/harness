use std::path::Path;
use std::process::Command;

/// Check if there are uncommitted changes in the repository.
///
/// Returns true iff `git status --porcelain` output is non-empty.
/// Covers staged, unstaged, and untracked changes.
#[allow(dead_code)]
pub fn uncommitted_changes(repo: &Path) -> bool {
    let output = Command::new("git")
        .arg("status")
        .arg("--porcelain")
        .current_dir(repo)
        .output();

    match output {
        Ok(o) => !String::from_utf8_lossy(&o.stdout).trim().is_empty(),
        Err(_) => false,
    }
}

/// Get local branches matching `condukt/*` that are not merged into `main`.
///
/// Uses `git branch --no-merged main --list "condukt/*" --format "%(refname:short)"`.
/// Returns empty vec on command failure (fail-soft).
#[allow(dead_code)]
pub fn unmerged_condukt_branches(repo: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("branch")
        .arg("--no-merged")
        .arg("main")
        .arg("--list")
        .arg("condukt/*")
        .arg("--format")
        .arg("%(refname:short)")
        .current_dir(repo)
        .output();

    match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect(),
        Err(_) => vec![],
    }
}

/// Get worktree paths other than the main repo path.
///
/// Parses `git worktree list --porcelain` and collects paths that are not the main repo.
/// Returns empty vec on failure (fail-soft).
#[allow(dead_code)]
pub fn leftover_worktrees(repo: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("worktree")
        .arg("list")
        .arg("--porcelain")
        .current_dir(repo)
        .output();

    let main_repo_path = match std::fs::canonicalize(repo) {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let mut result = vec![];

            for line in stdout.lines() {
                if line.starts_with("worktree ") {
                    if let Some(path_str) = line.strip_prefix("worktree ") {
                        let path = std::path::PathBuf::from(path_str);
                        if let Ok(canonical_path) = std::fs::canonicalize(&path) {
                            if canonical_path != main_repo_path {
                                result.push(path_str.to_string());
                            }
                        }
                    }
                }
            }

            result
        }
        Err(_) => vec![],
    }
}

/// Count commits ahead of upstream.
///
/// Tries `git rev-list --count @{upstream}..HEAD`.
/// If that fails (no upstream configured), falls back to `git rev-list --count origin/main..HEAD`.
/// If that also fails, returns 0.
#[allow(dead_code)]
pub fn unpushed_count(repo: &Path) -> usize {
    // Try @{upstream}..HEAD first
    let output = Command::new("git")
        .arg("rev-list")
        .arg("--count")
        .arg("@{upstream}..HEAD")
        .current_dir(repo)
        .output();

    if let Ok(o) = output {
        if o.status.success() {
            if let Ok(count_str) = String::from_utf8(o.stdout) {
                if let Ok(count) = count_str.trim().parse::<usize>() {
                    return count;
                }
            }
        }
    }

    // Fall back to origin/main..HEAD
    let output = Command::new("git")
        .arg("rev-list")
        .arg("--count")
        .arg("origin/main..HEAD")
        .current_dir(repo)
        .output();

    if let Ok(o) = output {
        if o.status.success() {
            if let Ok(count_str) = String::from_utf8(o.stdout) {
                if let Ok(count) = count_str.trim().parse::<usize>() {
                    return count;
                }
            }
        }
    }

    // If both fail, return 0
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_git_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        let repo_path = dir.path();

        // Initialize git repo
        Command::new("git")
            .arg("init")
            .current_dir(repo_path)
            .output()
            .unwrap();

        // Set user config
        Command::new("git")
            .arg("-c")
            .arg("user.email=t@t")
            .arg("-c")
            .arg("user.name=t")
            .arg("config")
            .arg("user.email")
            .arg("t@t")
            .current_dir(repo_path)
            .output()
            .unwrap();

        Command::new("git")
            .arg("-c")
            .arg("user.email=t@t")
            .arg("-c")
            .arg("user.name=t")
            .arg("config")
            .arg("user.name")
            .arg("t")
            .current_dir(repo_path)
            .output()
            .unwrap();

        dir
    }

    #[test]
    fn test_uncommitted_changes() {
        let dir = setup_git_repo();
        let repo_path = dir.path();

        // Initially no changes
        assert!(!uncommitted_changes(repo_path));

        // Create an untracked file
        fs::write(repo_path.join("untracked.txt"), "content").unwrap();

        // Now there should be uncommitted changes
        assert!(uncommitted_changes(repo_path));

        // Add and commit
        Command::new("git")
            .arg("add")
            .arg("untracked.txt")
            .current_dir(repo_path)
            .output()
            .unwrap();

        Command::new("git")
            .arg("-c")
            .arg("user.email=t@t")
            .arg("-c")
            .arg("user.name=t")
            .arg("commit")
            .arg("-m")
            .arg("test commit")
            .current_dir(repo_path)
            .output()
            .unwrap();

        // After commit, no more uncommitted changes
        assert!(!uncommitted_changes(repo_path));
    }

    #[test]
    fn test_unpushed_count_no_upstream() {
        let dir = setup_git_repo();
        let repo_path = dir.path();

        // Create initial commit
        fs::write(repo_path.join("file.txt"), "content").unwrap();
        Command::new("git")
            .arg("add")
            .arg("file.txt")
            .current_dir(repo_path)
            .output()
            .unwrap();

        Command::new("git")
            .arg("-c")
            .arg("user.email=t@t")
            .arg("-c")
            .arg("user.name=t")
            .arg("commit")
            .arg("-m")
            .arg("initial commit")
            .current_dir(repo_path)
            .output()
            .unwrap();

        // With no upstream and no origin/main, should return 0 gracefully
        let count = unpushed_count(repo_path);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_leftover_worktrees_empty() {
        let dir = setup_git_repo();
        let repo_path = dir.path();

        // In a fresh repo with no worktrees, should return empty
        let worktrees = leftover_worktrees(repo_path);
        assert!(worktrees.is_empty());
    }

    #[test]
    fn test_unmerged_condukt_branches_empty() {
        let dir = setup_git_repo();
        let repo_path = dir.path();

        // Create initial commit on main
        fs::write(repo_path.join("file.txt"), "content").unwrap();
        Command::new("git")
            .arg("add")
            .arg("file.txt")
            .current_dir(repo_path)
            .output()
            .unwrap();

        Command::new("git")
            .arg("-c")
            .arg("user.email=t@t")
            .arg("-c")
            .arg("user.name=t")
            .arg("commit")
            .arg("-m")
            .arg("initial commit")
            .current_dir(repo_path)
            .output()
            .unwrap();

        // In a fresh repo with no unmerged condukt branches, should return empty
        let branches = unmerged_condukt_branches(repo_path);
        assert!(branches.is_empty());
    }
}
