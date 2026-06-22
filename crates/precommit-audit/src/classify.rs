//! File classification: which changed files are source, tests, or excluded.
//! Mirrors the original hook's Test-IsExcluded / Test-IsTestFile / Test-IsSource
//! helpers, but the extension lists and exclude patterns come from config.

use crate::config::Classify;
use regex::Regex;

pub struct Classifier<'a> {
    cfg: &'a Classify,
    test_re: Regex,
}

impl<'a> Classifier<'a> {
    pub fn new(cfg: &'a Classify) -> Self {
        // Combined test-path heuristic (matches the six PowerShell alternatives).
        let test_re = Regex::new(
            r"(?x)
              (^|/)tests?/
            | (^|/)test_[^/]+$
            | _test\.(py|ts|tsx|js|jsx)$
            | \.test\.(ts|tsx|js|jsx)$
            | (^|/)__tests__/
            | (^|/)e2e/
            ",
        )
        .expect("static test regex");
        Classifier { cfg, test_re }
    }

    pub fn is_excluded(&self, path: &str) -> bool {
        let p = norm(path);
        self.cfg.exclude.iter().any(|pat| p.contains(pat.as_str()))
    }

    pub fn is_test(&self, path: &str) -> bool {
        self.test_re.is_match(&norm(path))
    }

    pub fn is_source(&self, path: &str) -> bool {
        if self.is_excluded(path) || self.is_test(path) {
            return false;
        }
        match ext_of(path) {
            Some(e) => self.cfg.source_exts.iter().any(|x| x == &e),
            None => false,
        }
    }

    pub fn is_scannable(&self, path: &str) -> bool {
        if self.is_excluded(path) {
            return false;
        }
        match ext_of(path) {
            Some(e) => self.cfg.scan_exts.iter().any(|x| x == &e),
            None => false,
        }
    }
}

/// Normalize backslashes to forward slashes so globs/patterns are OS-agnostic.
pub fn norm(path: &str) -> String {
    path.replace('\\', "/")
}

/// Extension including the leading dot, lowercased (e.g. ".py"). None if absent.
pub fn ext_of(path: &str) -> Option<String> {
    let name = norm(path);
    let name = name.rsplit('/').next().unwrap_or(&name);
    match name.rfind('.') {
        Some(i) if i > 0 => Some(name[i..].to_ascii_lowercase()),
        _ => None,
    }
}
