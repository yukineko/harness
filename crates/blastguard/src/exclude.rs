//! "Is this path a repo config file we should never block?" — the allowlist.
//!
//! Editing or even deleting a project's config files (`Cargo.toml`, lockfiles,
//! `package.json`, `.claude/**`, …) is routine and must never be treated as a
//! destructive blast. This module centralizes that judgement plus the helpers
//! that pull candidate paths out of tool inputs and shell operands.

use std::sync::OnceLock;

use globset::{Glob, GlobSet, GlobSetBuilder};

/// Glob patterns whose matches are always allowed (treated as config files).
const ALLOW_GLOBS: &[&str] = &[
    // The Claude Code project config tree, anywhere in the repo.
    ".claude",
    ".claude/**",
    "**/.claude",
    "**/.claude/**",
    // Explicit settings files called out by the spec.
    "settings.local.json",
    "**/settings.local.json",
    "**/.claude/settings.json",
    // Package / manifest files.
    "package.json",
    "**/package.json",
    // Manifests & config formats by extension (basename and nested).
    "*.toml",
    "**/*.toml",
    "*.yaml",
    "*.yml",
    "**/*.yaml",
    "**/*.yml",
    "*.lock",
    "**/*.lock",
    // Dotted config trees.
    ".config",
    ".config/**",
    "**/.config",
    "**/.config/**",
];

fn glob_set() -> &'static GlobSet {
    static SET: OnceLock<GlobSet> = OnceLock::new();
    SET.get_or_init(|| {
        let mut b = GlobSetBuilder::new();
        for pat in ALLOW_GLOBS {
            // Patterns are compile-time constants; a build error would be a bug.
            if let Ok(g) = Glob::new(pat) {
                b.add(g);
            }
        }
        b.build().unwrap_or_else(|_| GlobSet::empty())
    })
}

/// Normalize a raw path/operand for matching: strip surrounding quotes,
/// convert backslashes to forward slashes, and drop a leading `./`.
pub fn normalize(raw: &str) -> String {
    let mut s = raw.trim();
    // Strip a single layer of matching surrounding quotes.
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        s = &s[1..s.len() - 1];
    }
    let mut s = s.replace('\\', "/");
    while let Some(rest) = s.strip_prefix("./") {
        s = rest.to_string();
    }
    s
}

/// True when `path` is a repo config file that must never be blocked.
pub fn is_config_file(path: &str) -> bool {
    let norm = normalize(path);
    if norm.is_empty() {
        return false;
    }
    glob_set().is_match(&norm)
}

/// True when `path` points inside a `.git/` directory (git internals). Wholesale
/// overwriting these is destructive even though they are not "config files".
pub fn is_git_internal(path: &str) -> bool {
    let norm = normalize(path);
    norm == ".git"
        || norm.starts_with(".git/")
        || norm.contains("/.git/")
        || norm.ends_with("/.git")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_extensions_are_excluded() {
        assert!(is_config_file("Cargo.toml"));
        assert!(is_config_file("crates/blastguard/Cargo.toml"));
        assert!(is_config_file("/abs/path/Cargo.toml"));
        assert!(is_config_file("pnpm-lock.yaml"));
        assert!(is_config_file("Cargo.lock"));
        assert!(is_config_file("package.json"));
        assert!(is_config_file("config.yml"));
        assert!(is_config_file("ci.yaml"));
    }

    #[test]
    fn claude_and_dot_config_trees_are_excluded() {
        assert!(is_config_file(".claude"));
        assert!(is_config_file(".claude/settings.json"));
        assert!(is_config_file(".claude/agents/foo.md"));
        assert!(is_config_file("nested/.claude/settings.json"));
        assert!(is_config_file("settings.local.json"));
        assert!(is_config_file(".config/foo/bar.conf"));
        assert!(is_config_file("home/.config/x.ini"));
    }

    #[test]
    fn quotes_and_dot_slash_are_normalized() {
        assert!(is_config_file("\"Cargo.toml\""));
        assert!(is_config_file("'Cargo.toml'"));
        assert!(is_config_file("./Cargo.toml"));
    }

    #[test]
    fn ordinary_source_is_not_a_config_file() {
        assert!(!is_config_file("src/main.rs"));
        assert!(!is_config_file("README.md"));
        assert!(!is_config_file("notes.txt"));
        assert!(!is_config_file("build"));
        assert!(!is_config_file("*"));
        assert!(!is_config_file(""));
    }

    #[test]
    fn git_internals_detected() {
        assert!(is_git_internal(".git/config"));
        assert!(is_git_internal("worktree/.git/HEAD"));
        assert!(!is_git_internal("src/git.rs"));
        assert!(!is_git_internal("gitignore"));
    }
}
