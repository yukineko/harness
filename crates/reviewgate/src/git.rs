//! git helpers: which files changed, and the actual diff text for them. Pure
//! subprocess calls to `git`. `None` from `changed_files` means "not a git repo
//! / git unavailable" — the caller then has no diff to review and allows the
//! stop.

use std::path::Path;
use std::process::Command;

/// Changed paths relative to the repo root: tracked changes vs HEAD, staged
/// changes, and untracked-but-not-ignored files. `None` ⇒ not a git repo.
pub fn changed_files(root: &Path) -> Option<Vec<String>> {
    if !is_git_repo(root) {
        return None;
    }
    let mut out = Vec::new();
    collect(root, &["diff", "--name-only"], &mut out);
    collect(root, &["diff", "--cached", "--name-only"], &mut out);
    collect(
        root,
        &["ls-files", "--others", "--exclude-standard"],
        &mut out,
    );
    out.sort();
    out.dedup();
    Some(out)
}

fn is_git_repo(root: &Path) -> bool {
    Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn collect(root: &Path, args: &[&str], out: &mut Vec<String>) {
    if let Ok(o) = Command::new("git").current_dir(root).args(args).output() {
        if o.status.success() {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let line = line.trim();
                if !line.is_empty() {
                    out.push(line.to_string());
                }
            }
        }
    }
}

/// The combined diff for `files`: unstaged + staged hunks, plus the full text of
/// any untracked files (which have no diff). Truncated to `max_bytes` with a
/// marker so a huge diff can't blow up memory or the reviewer prompt.
pub fn diff_text(root: &Path, files: &[String], max_bytes: usize) -> String {
    let mut s = String::new();

    run_diff(root, &["diff", "--"], files, &mut s);
    run_diff(root, &["diff", "--cached", "--"], files, &mut s);

    // untracked files among `files`: include their contents as "new file" diffs.
    let mut others = Vec::new();
    let mut args: Vec<&str> = vec!["ls-files", "--others", "--exclude-standard", "--"];
    args.extend(files.iter().map(String::as_str));
    if let Ok(o) = Command::new("git").current_dir(root).args(&args).output() {
        if o.status.success() {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let line = line.trim();
                if !line.is_empty() {
                    others.push(line.to_string());
                }
            }
        }
    }
    for f in others {
        s.push_str(&format!("\n=== new file: {f} ===\n"));
        if let Ok(content) = std::fs::read_to_string(root.join(&f)) {
            s.push_str(&content);
            if !s.ends_with('\n') {
                s.push('\n');
            }
        }
        if s.len() > max_bytes {
            break;
        }
    }

    truncate_on_boundary(s, max_bytes)
}

fn run_diff(root: &Path, base: &[&str], files: &[String], out: &mut String) {
    let mut args: Vec<&str> = base.to_vec();
    args.extend(files.iter().map(String::as_str));
    if let Ok(o) = Command::new("git").current_dir(root).args(&args).output() {
        if o.status.success() {
            out.push_str(&String::from_utf8_lossy(&o.stdout));
        }
    }
}

fn truncate_on_boundary(mut s: String, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s;
    }
    let mut cut = max_bytes;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s.truncate(cut);
    s.push_str("\n… (diff truncated by reviewgate max_diff_bytes)\n");
    s
}
