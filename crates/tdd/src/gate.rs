//! The test-existence gate: did this change add implementation code without a
//! test? Deterministic — it reads git, never runs the suite (that's donegate's
//! job). The verdict drives whether the Stop hook blocks.

use std::path::Path;

use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::RegexSet;

use crate::config::Config;
use crate::git::{self, AddedLine};

pub struct Verdict {
    /// Number of *added* implementation lines (impl-glob file, not a test file,
    /// not itself a test marker).
    pub added_impl_lines: usize,
    /// An added line matched a test marker (e.g. an inline `#[test]`).
    pub test_marker_added: bool,
    /// A changed/added file is a test by location/name.
    pub test_file_changed: bool,
    /// A few implementation files touched, for the message.
    pub impl_files: Vec<String>,
    /// True when there is no git repo, so we can't scope and allow the stop.
    pub git_unscoped: bool,
}

impl Verdict {
    pub fn has_test_evidence(&self) -> bool {
        self.test_marker_added || self.test_file_changed
    }

    /// Block the stop iff enough new implementation landed with no test.
    pub fn blocks(&self, cfg: &Config) -> bool {
        !self.git_unscoped
            && self.added_impl_lines >= cfg.min_added_impl_lines.max(1)
            && !self.has_test_evidence()
    }
}

fn build_globset(globs: &[String]) -> GlobSet {
    let mut b = GlobSetBuilder::new();
    for g in globs {
        if let Ok(glob) = Glob::new(g) {
            b.add(glob);
        }
    }
    b.build().unwrap_or_else(|_| GlobSet::empty())
}

fn build_markers(patterns: &[String]) -> RegexSet {
    RegexSet::new(patterns).unwrap_or_else(|_| {
        // Drop individually-invalid patterns rather than disabling all markers.
        let good: Vec<&String> = patterns
            .iter()
            .filter(|p| regex::Regex::new(p).is_ok())
            .collect();
        RegexSet::new(good).unwrap_or_else(|_| RegexSet::empty())
    })
}

/// Classify a set of changed files + added lines into a verdict. Pure (no git):
/// unit-testable.
pub fn classify(
    cfg: &Config,
    changed: &Option<Vec<String>>,
    added: &Option<Vec<AddedLine>>,
) -> Verdict {
    let impl_set = build_globset(&cfg.impl_globs);
    let test_set = build_globset(&cfg.test_path_globs);
    let markers = build_markers(&cfg.test_markers);

    let (Some(changed), Some(added)) = (changed, added) else {
        return Verdict {
            added_impl_lines: 0,
            test_marker_added: false,
            test_file_changed: false,
            impl_files: Vec::new(),
            git_unscoped: true,
        };
    };

    let is_test_file = |f: &str| test_set.is_match(f);
    let is_impl_file = |f: &str| impl_set.is_match(f) && !is_test_file(f);

    let test_file_changed = changed.iter().any(|f| is_test_file(f));

    let mut added_impl_lines = 0usize;
    let mut test_marker_added = false;
    let mut impl_files: Vec<String> = Vec::new();
    for AddedLine { file, text } in added {
        let marker_hit = markers.is_match(text);
        if is_test_file(file) {
            test_marker_added = true; // any added line in a test file is evidence
            continue;
        }
        if marker_hit {
            test_marker_added = true; // inline test written in an impl file
            continue;
        }
        if is_impl_file(file) && !text.trim().is_empty() {
            added_impl_lines += 1;
            if !impl_files.contains(file) {
                impl_files.push(file.clone());
            }
        }
    }

    Verdict {
        added_impl_lines,
        test_marker_added,
        test_file_changed,
        impl_files,
        git_unscoped: false,
    }
}

/// Run the gate against a real project root.
pub fn evaluate(cfg: &Config, root: &Path) -> Verdict {
    let changed = git::changed_files(root);
    let added = git::added_lines(root);
    classify(cfg, &changed, &added)
}

/// The reason injected back into the model when the stop is blocked.
pub fn block_reason(v: &Verdict, attempt: u32, max: u32) -> String {
    let sample = if v.impl_files.is_empty() {
        String::new()
    } else {
        let shown: Vec<&str> = v.impl_files.iter().take(6).map(String::as_str).collect();
        format!("\n  implementation changed: {}", shown.join(", "))
    };
    format!(
        "🔴 tdd: write a test first — {} new implementation line(s) added with no \
         accompanying test (attempt {attempt}/{max}).{sample}\n\n\
         Add a test that exercises this change (a `#[test]`, `def test_…`, `func Test…`, \
         `it(...)`, or a file under tests/), then finish. Prefer test-first: run \
         `tdd red --task <id>` to capture the failing test before you implement, and \
         `tdd green --task <id>` once it passes.\n\n\
         Genuinely no test needed (pure refactor/rename/docs)? Create `.tdd-skip` in the \
         project root with a one-line reason (consumed once). Disable entirely: TDD_DISABLE=1.",
        v.added_impl_lines
    )
}

/// Compact human report for manual `tdd gate` / `tdd status` runs.
pub fn human_report(v: &Verdict, cfg: &Config) -> String {
    if v.git_unscoped {
        return "(no git repo — tdd gate allows the stop)".to_string();
    }
    let mut s = String::new();
    s.push_str(&format!("added impl lines: {}\n", v.added_impl_lines));
    s.push_str(&format!(
        "test evidence:    {}\n",
        if v.has_test_evidence() {
            if v.test_file_changed && v.test_marker_added {
                "yes (test file + inline test)"
            } else if v.test_file_changed {
                "yes (test file changed)"
            } else {
                "yes (inline test added)"
            }
        } else {
            "none"
        }
    ));
    if v.blocks(cfg) {
        s.push_str("\n🔴 would BLOCK: implementation added without a test");
    } else {
        s.push_str("\n✓ would allow the stop");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn added(file: &str, text: &str) -> AddedLine {
        AddedLine {
            file: file.to_string(),
            text: text.to_string(),
        }
    }

    #[test]
    fn impl_without_test_blocks() {
        let cfg = Config::default();
        let changed = Some(vec!["src/lib.rs".to_string()]);
        let added = Some(vec![added("src/lib.rs", "pub fn add(a:i32,b:i32)->i32{a+b}")]);
        let v = classify(&cfg, &changed, &added);
        assert_eq!(v.added_impl_lines, 1);
        assert!(!v.has_test_evidence());
        assert!(v.blocks(&cfg));
    }

    #[test]
    fn inline_test_is_evidence() {
        let cfg = Config::default();
        let changed = Some(vec!["src/lib.rs".to_string()]);
        let added = Some(vec![
            added("src/lib.rs", "pub fn add(a:i32,b:i32)->i32{a+b}"),
            added("src/lib.rs", "    #[test]"),
            added("src/lib.rs", "    fn test_add() { assert_eq!(add(1,2),3); }"),
        ]);
        let v = classify(&cfg, &changed, &added);
        assert!(v.test_marker_added);
        assert!(!v.blocks(&cfg));
    }

    #[test]
    fn separate_test_file_is_evidence() {
        let cfg = Config::default();
        let changed = Some(vec![
            "src/lib.rs".to_string(),
            "tests/add_test.rs".to_string(),
        ]);
        let added = Some(vec![
            added("src/lib.rs", "pub fn add(a:i32,b:i32)->i32{a+b}"),
            added("tests/add_test.rs", "assert_eq!(add(1,2),3);"),
        ]);
        let v = classify(&cfg, &changed, &added);
        assert!(v.test_file_changed);
        assert!(!v.blocks(&cfg));
    }

    #[test]
    fn docs_only_change_allowed() {
        let cfg = Config::default();
        let changed = Some(vec!["README.md".to_string()]);
        let added = Some(vec![added("README.md", "# hello")]);
        let v = classify(&cfg, &changed, &added);
        assert_eq!(v.added_impl_lines, 0);
        assert!(!v.blocks(&cfg));
    }

    #[test]
    fn no_git_never_blocks() {
        let cfg = Config::default();
        let v = classify(&cfg, &None, &None);
        assert!(v.git_unscoped);
        assert!(!v.blocks(&cfg));
    }

    #[test]
    fn blank_added_lines_dont_count() {
        let cfg = Config::default();
        let changed = Some(vec!["src/lib.rs".to_string()]);
        let added = Some(vec![added("src/lib.rs", "   ")]);
        let v = classify(&cfg, &changed, &added);
        assert_eq!(v.added_impl_lines, 0);
        assert!(!v.blocks(&cfg));
    }
}
