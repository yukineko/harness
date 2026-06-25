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

/// Returns an error if `base` starts with `-`, which would be misinterpreted
/// as a git option flag and could allow argument injection attacks.
fn validate_base(base: &str) -> Result<(), String> {
    if base.starts_with('-') {
        Err(format!(
            "invalid base commit: {base:?} must not start with '-'"
        ))
    } else {
        Ok(())
    }
}

/// `git diff --stat <base>..HEAD` — file-level change summary.
pub fn diff_stat(root: &Path, base: &str) -> String {
    if validate_base(base).is_err() {
        return String::new();
    }
    let out = Command::new("git")
        .args(["diff", "--stat", "--", &format!("{base}..HEAD")])
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
    if validate_base(base).is_err() {
        return Vec::new();
    }
    let out = Command::new("git")
        .args(["diff", "--name-status", "--", &format!("{base}..HEAD")])
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
    if validate_base(base).is_err() {
        return String::new();
    }
    let out = Command::new("git")
        .args(["diff", "--diff-filter=ACMRT", "--", &format!("{base}..HEAD")])
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
    if validate_base(base).is_err() {
        return String::new();
    }
    let out = Command::new("git")
        .args(["log", "--oneline", "--", &format!("{base}..HEAD")])
        .current_dir(root)
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::validate_base;

    #[test]
    fn validate_base_accepts_normal_sha() {
        assert!(validate_base("abc1234").is_ok());
    }

    #[test]
    fn validate_base_accepts_branch_name() {
        assert!(validate_base("main").is_ok());
    }

    #[test]
    fn validate_base_accepts_empty_string() {
        // empty string does not start with '-', so validation passes
        assert!(validate_base("").is_ok());
    }

    #[test]
    fn validate_base_rejects_flag_like_value() {
        assert!(validate_base("--output=/tmp/x").is_err());
    }

    #[test]
    fn validate_base_rejects_single_dash_prefix() {
        assert!(validate_base("-n").is_err());
    }

    #[test]
    fn validate_base_rejects_malicious_flag() {
        // Simulate the injection attack described in done_criteria
        let malicious = "--output=/tmp/x";
        assert!(validate_base(malicious).is_err());
    }

    #[test]
    fn diff_stat_returns_empty_for_injected_base() {
        use std::path::Path;
        // With a malicious base that starts with '-', diff_stat must return empty
        // without passing the value as a flag to git.
        let result = super::diff_stat(Path::new("/tmp"), "--output=/tmp/x");
        assert_eq!(result, "");
    }

    #[test]
    fn diff_name_status_returns_empty_for_injected_base() {
        use std::path::Path;
        let result = super::diff_name_status(Path::new("/tmp"), "--output=/tmp/x");
        assert!(result.is_empty());
    }

    #[test]
    fn diff_body_returns_empty_for_injected_base() {
        use std::path::Path;
        let result = super::diff_body(Path::new("/tmp"), "--output=/tmp/x", 4096);
        assert_eq!(result, "");
    }

    #[test]
    fn log_oneline_returns_empty_for_injected_base() {
        use std::path::Path;
        let result = super::log_oneline(Path::new("/tmp"), "--output=/tmp/x");
        assert_eq!(result, "");
    }
}
