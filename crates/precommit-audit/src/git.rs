//! Thin wrappers over the `git` CLI. We shell out rather than link libgit2 so
//! the binary stays small and matches the exact semantics of the original hook
//! (which also shelled out). All output is decoded as UTF-8 (lossy) — git emits
//! UTF-8 on every platform, which sidesteps the CP932 mojibake the PowerShell
//! version had to guard against.

use std::path::Path;
use std::process::Command;

/// Run `git <args>` in `cwd`, returning stdout as a UTF-8 string. stderr is
/// discarded; a non-zero exit yields whatever stdout was produced (often empty).
fn git(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => String::new(),
    }
}

/// Files changed vs HEAD plus untracked files (excluding standard ignores),
/// de-duplicated and sorted. This is the audit's working set.
pub fn changed_and_untracked(cwd: &Path) -> Vec<String> {
    let mut set: Vec<String> = Vec::new();
    for src in [
        git(cwd, &["diff", "--name-only", "HEAD"]),
        git(cwd, &["ls-files", "--others", "--exclude-standard"]),
    ] {
        for line in src.lines() {
            let l = line.trim_end_matches('\r');
            if !l.is_empty() {
                set.push(l.to_string());
            }
        }
    }
    set.sort();
    set.dedup();
    set
}

/// Untracked files only (used by the diff-hash computation).
pub fn untracked(cwd: &Path) -> Vec<String> {
    git(cwd, &["ls-files", "--others", "--exclude-standard"])
        .lines()
        .map(|l| l.trim_end_matches('\r').to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Full `git diff HEAD` over the whole tree (used by the diff-hash computation).
pub fn diff_head(cwd: &Path) -> String {
    git(cwd, &["diff", "HEAD"])
}

/// Unified diff for a single file. Falls back to `--no-index` against /dev/null
/// for untracked files so their full content shows up as added lines.
pub fn file_diff(cwd: &Path, file: &str) -> String {
    let d = git(cwd, &["diff", "HEAD", "--", file]);
    if !d.trim().is_empty() {
        return d;
    }
    // Untracked: diff against the empty device. `--no-index` exits non-zero when
    // files differ, but we only consume stdout, so that is fine.
    git(cwd, &["diff", "--no-index", "/dev/null", file])
}

/// Added lines of a file's diff: lines starting with `+` (but not `+++`), with
/// any `audit-ignore: <reason>` line removed. The leading `+` is preserved so
/// pattern regexes anchored at `^\+` keep working.
pub fn added_lines(cwd: &Path, file: &str) -> Vec<String> {
    let diff = file_diff(cwd, file);
    diff.lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .filter(|l| !has_audit_ignore(l))
        .map(|l| l.to_string())
        .collect()
}

/// `git grep -l -E --untracked -- <pattern>`: files (tracked + untracked)
/// containing a line matching the regex. Empty on no match / no repo.
pub fn grep_files(cwd: &Path, pattern: &str) -> Vec<String> {
    git(cwd, &["grep", "-l", "-E", "--untracked", "--", pattern])
        .lines()
        .map(|l| l.trim_end_matches('\r').to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// `git rev-parse --show-toplevel`: the repo root, or None outside a repo.
pub fn toplevel(cwd: &Path) -> Option<String> {
    let s = git(cwd, &["rev-parse", "--show-toplevel"]);
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// True if the line carries a reasoned `audit-ignore:` marker. A bare
/// `audit-ignore` without a following non-space char does NOT suppress.
pub fn has_audit_ignore(line: &str) -> bool {
    if let Some(idx) = line.find("audit-ignore:") {
        let rest = &line[idx + "audit-ignore:".len()..];
        rest.trim_start().chars().next().is_some()
    } else {
        false
    }
}
