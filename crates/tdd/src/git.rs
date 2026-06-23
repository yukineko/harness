//! Talk to `git` to learn what changed: the set of changed paths (for test-file
//! evidence) and the *added* lines with their file (for inline-test detection
//! and counting newly added implementation lines). Pure subprocess calls.

use std::path::Path;
use std::process::Command;

/// One added line in the working tree's diff, tagged with the file it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedLine {
    pub file: String,
    pub text: String,
}

fn is_git_repo(root: &Path) -> bool {
    Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run(root: &Path, args: &[&str]) -> Option<String> {
    let o = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .ok()?;
    if o.status.success() {
        Some(String::from_utf8_lossy(&o.stdout).into_owned())
    } else {
        None
    }
}

/// Changed paths relative to the repo root: tracked changes vs HEAD, staged
/// changes, and untracked-but-not-ignored files. `None` means "not a git repo".
pub fn changed_files(root: &Path) -> Option<Vec<String>> {
    if !is_git_repo(root) {
        return None;
    }
    let mut out = Vec::new();
    for args in [
        &["diff", "--name-only"][..],
        &["diff", "--cached", "--name-only"][..],
        &["ls-files", "--others", "--exclude-standard"][..],
    ] {
        if let Some(text) = run(root, args) {
            for line in text.lines() {
                let line = line.trim();
                if !line.is_empty() {
                    out.push(line.to_string());
                }
            }
        }
    }
    out.sort();
    out.dedup();
    Some(out)
}

/// All *added* lines across unstaged + staged diffs, plus the full content of
/// untracked files (which have no diff). `None` means "not a git repo".
pub fn added_lines(root: &Path) -> Option<Vec<AddedLine>> {
    if !is_git_repo(root) {
        return None;
    }
    let mut out = Vec::new();
    for args in [&["diff", "-U0"][..], &["diff", "--cached", "-U0"][..]] {
        if let Some(text) = run(root, args) {
            parse_unified_diff(&text, &mut out);
        }
    }
    // Untracked files: every line is "added".
    if let Some(text) = run(root, &["ls-files", "--others", "--exclude-standard"]) {
        for f in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            if let Ok(content) = std::fs::read_to_string(root.join(f)) {
                for line in content.lines() {
                    out.push(AddedLine {
                        file: f.to_string(),
                        text: line.to_string(),
                    });
                }
            }
        }
    }
    Some(out)
}

/// Parse `git diff` unified output, collecting `+` lines tagged with the file
/// from the most recent `+++ b/<path>` header. `+++`/`---` markers are skipped.
pub fn parse_unified_diff(diff: &str, out: &mut Vec<AddedLine>) {
    let mut current: Option<String> = None;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("+++ ") {
            current = Some(strip_diff_prefix(rest));
            continue;
        }
        if line.starts_with("--- ") || line.starts_with("+++") {
            continue;
        }
        if let Some(added) = line.strip_prefix('+') {
            if let Some(file) = &current {
                if file != "/dev/null" {
                    out.push(AddedLine {
                        file: file.clone(),
                        text: added.to_string(),
                    });
                }
            }
        }
    }
}

/// `b/src/main.rs` → `src/main.rs`; `/dev/null` stays as-is.
fn strip_diff_prefix(path: &str) -> String {
    let p = path.trim();
    if p == "/dev/null" {
        return p.to_string();
    }
    p.strip_prefix("a/")
        .or_else(|| p.strip_prefix("b/"))
        .unwrap_or(p)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_added_lines_with_file() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -0,0 +1,2 @@
+pub fn add(a: i32, b: i32) -> i32 { a + b }
+// note
diff --git a/old.rs b/old.rs
--- a/old.rs
+++ /dev/null
@@ -1 +0,0 @@
-gone
";
        let mut out = Vec::new();
        parse_unified_diff(diff, &mut out);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|l| l.file == "src/lib.rs"));
        assert!(out[0].text.contains("pub fn add"));
    }
}
