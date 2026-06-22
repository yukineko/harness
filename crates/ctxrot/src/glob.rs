//! A tiny, dependency-free glob matcher (built on the `regex` crate already in
//! the tree) for the load allow/deny rules. We deliberately avoid pulling in a
//! `glob`/`globset` crate: the surface we need is small and must keep building
//! offline (the plugin ships a committed binary, no `cargo fetch` at install).
//!
//! Supported syntax (POSIX-ish, path-aware):
//!   * `*`   — any run of non-separator chars
//!   * `?`   — exactly one non-separator char
//!   * `**`  — any run including `/` (recursive)
//!   * `**/` — zero or more leading directory components (so `**/x` matches `x`)
//!
//! Matching is tried against several spellings of the candidate path (the raw
//! string the model passed, the resolved absolute path, the path relative to the
//! project root, and the bare file name for slash-less patterns), so an
//! intuitive pattern like `secrets/**` or `*.log` matches whether the model used
//! a relative or absolute path.

use std::path::Path;

use regex::Regex;

/// Convert a glob to an anchored regex source string.
fn glob_to_regex(glob: &str) -> String {
    let bytes = glob.as_bytes();
    let mut re = String::from("^");
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        match c {
            '*' => {
                let double = i + 1 < bytes.len() && bytes[i + 1] == b'*';
                if double {
                    i += 1; // consume the second '*'
                    if i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                        i += 1; // consume the trailing '/'
                        re.push_str("(?:.*/)?"); // `**/` → optional dir prefix
                    } else {
                        re.push_str(".*"); // `**` → anything, incl. separators
                    }
                } else {
                    re.push_str("[^/]*"); // `*` → within one path segment
                }
            }
            '?' => re.push_str("[^/]"),
            // Escape every regex metacharacter so the rest is literal.
            '.' | '+' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$' | '\\' => {
                re.push('\\');
                re.push(c);
            }
            other => re.push(other),
        }
        i += 1;
    }
    re.push('$');
    re
}

/// True if `pattern` matches `raw` (the model's spelling), `resolved` (the
/// absolute path we sized), or — when `pattern` has no `/` — the bare file name.
/// When `project_root` is a prefix of `resolved`, the project-relative path is
/// also tried, so `secrets/**` matches `/abs/proj/secrets/x`.
pub fn matches(pattern: &str, raw: &str, resolved: &Path, project_root: Option<&Path>) -> bool {
    let re = match Regex::new(&glob_to_regex(pattern)) {
        Ok(re) => re,
        Err(_) => return false, // a malformed pattern never matches (fail-open)
    };

    let norm = |s: &str| s.replace('\\', "/");
    let resolved_s = norm(&resolved.to_string_lossy());
    let mut candidates: Vec<String> = vec![norm(raw), resolved_s.clone()];

    if let Some(root) = project_root {
        let root_s = norm(&root.to_string_lossy());
        let root_s = root_s.trim_end_matches('/');
        if let Some(rel) = resolved_s.strip_prefix(root_s) {
            candidates.push(rel.trim_start_matches('/').to_string());
        }
    }

    if !pattern.contains('/') {
        if let Some(name) = resolved.file_name().and_then(|n| n.to_str()) {
            candidates.push(name.to_string());
        }
    }

    candidates.iter().any(|c| re.is_match(c))
}

/// True if any pattern in `patterns` matches. Empty list → false.
pub fn any_match(
    patterns: &[String],
    raw: &str,
    resolved: &Path,
    project_root: Option<&Path>,
) -> bool {
    patterns
        .iter()
        .any(|p| matches(p, raw, resolved, project_root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn slashless_pattern_matches_basename_anywhere() {
        // A pattern with no `/` is matched against the bare file name too, so
        // `*.log` catches a `.log` file in any directory (intuitive for deny).
        assert!(matches("*.log", "a.log", &p("/proj/a.log"), None));
        assert!(matches("*.log", "sub/a.log", &p("/proj/sub/a.log"), None));
    }

    #[test]
    fn star_does_not_cross_separator_in_path_match() {
        // With a `/` in the pattern there is no basename fallback, so a single
        // `*` stays within one segment: `dir/*.log` matches `dir/a.log` but not
        // a file one level deeper.
        assert!(matches("dir/*.log", "dir/a.log", &p("/proj/dir/a.log"), Some(&p("/proj"))));
        assert!(!matches(
            "dir/*.log",
            "dir/sub/a.log",
            &p("/proj/dir/sub/a.log"),
            Some(&p("/proj"))
        ));
    }

    #[test]
    fn doublestar_crosses_separators() {
        let f = p("/proj/node_modules/x/y.js");
        assert!(matches(
            "**/node_modules/**",
            "node_modules/x/y.js",
            &f,
            Some(&p("/proj"))
        ));
    }

    #[test]
    fn doublestar_slash_is_optional_prefix() {
        // `**/x.txt` matches both `x.txt` at root and nested.
        assert!(matches("**/x.txt", "x.txt", &p("/proj/x.txt"), Some(&p("/proj"))));
        assert!(matches(
            "**/x.txt",
            "deep/dir/x.txt",
            &p("/proj/deep/dir/x.txt"),
            Some(&p("/proj"))
        ));
    }

    #[test]
    fn project_relative_pattern_matches_absolute_path() {
        // `secrets/**` (project-relative) matches an absolute resolved path.
        assert!(matches(
            "secrets/**",
            "/proj/secrets/key.pem",
            &p("/proj/secrets/key.pem"),
            Some(&p("/proj"))
        ));
        assert!(!matches(
            "secrets/**",
            "/proj/src/main.rs",
            &p("/proj/src/main.rs"),
            Some(&p("/proj"))
        ));
    }

    #[test]
    fn extension_and_dir_patterns() {
        assert!(matches("**/*.min.js", "a/b.min.js", &p("/x/a/b.min.js"), Some(&p("/x"))));
        assert!(!matches("**/*.min.js", "a/b.js", &p("/x/a/b.js"), Some(&p("/x"))));
    }

    #[test]
    fn question_mark_matches_one_char() {
        assert!(matches("a?.txt", "ab.txt", &p("/x/ab.txt"), None));
        assert!(!matches("a?.txt", "abc.txt", &p("/x/abc.txt"), None));
    }

    #[test]
    fn any_match_is_or() {
        let pats = vec!["**/*.log".to_string(), "secrets/**".to_string()];
        assert!(any_match(&pats, "x.log", &p("/p/x.log"), Some(&p("/p"))));
        assert!(any_match(&pats, "/p/secrets/a", &p("/p/secrets/a"), Some(&p("/p"))));
        assert!(!any_match(&pats, "/p/src/main.rs", &p("/p/src/main.rs"), Some(&p("/p"))));
    }

    #[test]
    fn malformed_pattern_never_matches() {
        // unbalanced bracket would be a regex error in a naive impl; our escaper
        // makes it literal, so it simply won't match a normal path.
        assert!(!matches("[unclosed", "a.txt", &p("/x/a.txt"), None));
    }
}
