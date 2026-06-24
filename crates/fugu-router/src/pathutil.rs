//! Utilities for normalising file paths at record time.
//!
//! The key operation: if an absolute path is under a detectable git repo root,
//! strip the prefix so the stored path is repo-relative. Relative paths and
//! paths outside the repo root are left unchanged.
//!
//! Repo-relative paths transfer cleanly across machines (no username leakage)
//! and produce better k-NN similarity because `file_tokens` splits on `/` —
//! absolute paths would inject meaningless machine-specific segments.

use std::path::{Path, PathBuf};

/// Walk up from `start` looking for a `.git` directory. Returns the first
/// ancestor (inclusive) that contains `.git`, or `None`.
pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    // Canonicalize so symlinks don't confuse the prefix check later.
    if let Ok(c) = std::fs::canonicalize(&cur) {
        cur = c;
    }
    loop {
        if cur.join(".git").exists() {
            return Some(cur);
        }
        match cur.parent() {
            Some(p) => cur = p.to_path_buf(),
            None => return None,
        }
    }
}

/// Detect the repo root from the current working directory.
pub fn repo_root_from_cwd() -> Option<PathBuf> {
    find_repo_root(&std::env::current_dir().ok()?)
}

/// Normalise a single file path: if it is absolute and falls under `repo_root`,
/// return the repo-relative path (without a leading `/`). Otherwise return the
/// path unchanged.
///
/// Accepts `repo_root = None` to skip normalisation (e.g. not in a git repo).
pub fn normalise_path(raw: &str, repo_root: Option<&Path>) -> String {
    let Some(root) = repo_root else {
        return raw.to_string();
    };
    let p = Path::new(raw);
    if !p.is_absolute() {
        return raw.to_string();
    }
    // Canonicalize the input so symlinks don't prevent prefix stripping.
    let canonical = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    match canonical.strip_prefix(root) {
        Ok(rel) => rel.to_string_lossy().into_owned(),
        Err(_) => raw.to_string(),
    }
}

/// Normalise a list of file paths in place (see [`normalise_path`]).
pub fn normalise_paths(paths: &[String], repo_root: Option<&Path>) -> Vec<String> {
    paths.iter().map(|p| normalise_path(p, repo_root)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_repo_root_from_absolute_path() {
        // Use a non-existent path so canonicalize fails and the strip is purely
        // textual & deterministic (canonicalize is identity here anyway, but this
        // makes the assertion exact rather than "either-or").
        let root = PathBuf::from("/no/such/repo/root");
        let raw = "/no/such/repo/root/crates/x.rs";
        assert_eq!(normalise_path(raw, Some(&root)), "crates/x.rs");
    }

    #[test]
    fn no_op_when_already_relative() {
        let root = PathBuf::from("/Users/yuki/src/harness");
        let raw = "crates/x.rs";
        assert_eq!(normalise_path(raw, Some(&root)), "crates/x.rs");
    }

    #[test]
    fn no_op_when_outside_repo() {
        let root = PathBuf::from("/Users/yuki/src/harness");
        let raw = "/etc/passwd";
        assert_eq!(normalise_path(raw, Some(&root)), "/etc/passwd");
    }

    #[test]
    fn no_op_when_no_repo_root() {
        let raw = "/Users/yuki/src/harness/crates/x.rs";
        assert_eq!(normalise_path(raw, None), raw);
    }

    #[test]
    fn find_repo_root_finds_git_dir() {
        // The harness repo itself is a git repo; walking up from any subdir
        // should find it.
        let subdir = PathBuf::from("/Users/yuki/src/harness/crates/fugu-router/src");
        if subdir.exists() {
            let root = find_repo_root(&subdir);
            assert!(root.is_some(), "should have found a .git dir");
            assert!(root.unwrap().join(".git").exists());
        }
    }

    #[test]
    fn normalise_paths_batch() {
        let root = PathBuf::from("/repo");
        let paths = vec![
            "/repo/src/main.rs".to_string(),
            "relative/path.rs".to_string(),
            "/other/place.rs".to_string(),
        ];
        let result = normalise_paths(&paths, Some(&root));
        // relative stays as-is, outside-repo stays as-is
        assert_eq!(result[1], "relative/path.rs");
        assert_eq!(result[2], "/other/place.rs");
        // /repo/src/main.rs -> src/main.rs (non-existent path → deterministic strip)
        assert_eq!(result[0], "src/main.rs");
    }
}
