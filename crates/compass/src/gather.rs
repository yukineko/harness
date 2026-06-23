//! gather (DESIGN §3) — build a [`Bundle`] of provenance-tagged [`Fragment`]s
//! from the project's "current goal vs. what it's actually doing now" sources.
//!
//! Sources and their authority (a presentation HINT only — never auto-resolved):
//!   - the **charter** itself — [`Authority::High`] (the current stated goal),
//!   - **recent git activity** (commit subjects + uncommitted changes) —
//!     [`Authority::Mid`] (what the project is actually doing now),
//!   - **taskprog** `.claude/progress.md` / `progress.md` — [`Authority::Mid`],
//!   - **deepwiki** `.deepwiki/**/*.md` — [`Authority::Mid`].
//!
//! Gathering is deterministic and tolerant: a non-git repo or a missing file
//! simply contributes no fragments (the source is skipped, never an error).
//! `score` is a cheap length/recency heuristic — it carries no domain judgment.

use std::path::Path;
use std::process::Command;

use anyhow::Result;
use harness_core::interrogate::{Authority, Bundle, Fragment};

use crate::charter::Charter;
use crate::config::Config;

/// How many recent commit subjects to pull into the bundle.
const RECENT_COMMITS: usize = 30;

/// Build the gather [`Bundle`] for `repo_root`. Never errors on a missing
/// source; the `Result` is reserved for genuinely unexpected failures (none of
/// the current sources produce one — they all degrade to "no fragments").
// Wired into the `gap`/`route` commands in a later task (DESIGN §3); exercised
// by tests now.
#[allow(dead_code)]
pub fn gather(repo_root: &Path, charter: &Charter, _cfg: &Config) -> Result<Bundle> {
    let mut fragments: Vec<Fragment> = Vec::new();

    gather_charter(&mut fragments, charter);
    gather_git(&mut fragments, repo_root);
    gather_progress(&mut fragments, repo_root);
    gather_deepwiki(&mut fragments, repo_root);

    Ok(Bundle { fragments })
}

/// The charter is the current stated goal: High authority, one fragment per
/// populated field so downstream gates can reference fields individually.
fn gather_charter(out: &mut Vec<Fragment>, charter: &Charter) {
    let src = ".compass/charter.md";
    if !charter.north_star.is_empty() {
        out.push(Fragment {
            text: charter.north_star.clone(),
            source_path: src.to_string(),
            authority: Authority::High,
            score: charter.north_star.len() as i64,
            anchor: Some("north_star".to_string()),
        });
    }
    for (i, dod) in charter.definition_of_done.iter().enumerate() {
        out.push(Fragment {
            text: dod.clone(),
            source_path: src.to_string(),
            authority: Authority::High,
            score: dod.len() as i64,
            anchor: Some(format!("definition_of_done[{i}]")),
        });
    }
    if !charter.next_action.is_empty() {
        out.push(Fragment {
            text: charter.next_action.clone(),
            source_path: src.to_string(),
            authority: Authority::High,
            score: charter.next_action.len() as i64,
            anchor: Some("next_action".to_string()),
        });
    }
}

/// Recent git activity: last N commit subjects + the uncommitted change summary.
/// Mid authority — "what the project is actually doing now". Non-git dirs skip.
fn gather_git(out: &mut Vec<Fragment>, repo_root: &Path) {
    // Last N commit subjects, newest first. `score` decays with recency rank so
    // the newest commit ranks highest.
    if let Some(log) = git_stdout(repo_root, &["log", "--oneline", "-n", &RECENT_COMMITS.to_string()])
    {
        for (rank, line) in log.lines().filter(|l| !l.trim().is_empty()).enumerate() {
            out.push(Fragment {
                text: line.trim().to_string(),
                source_path: "git:log".to_string(),
                authority: Authority::Mid,
                // newest (rank 0) gets the highest score.
                score: (RECENT_COMMITS as i64 - rank as i64).max(0),
                anchor: Some("git log --oneline".to_string()),
            });
        }
    }

    // Uncommitted changes: short status (changed paths) + diffstat summary.
    if let Some(status) = git_stdout(repo_root, &["status", "--porcelain"]) {
        let status = status.trim();
        if !status.is_empty() {
            out.push(Fragment {
                text: status.to_string(),
                source_path: "git:status".to_string(),
                authority: Authority::Mid,
                score: status.lines().count() as i64,
                anchor: Some("git status --porcelain".to_string()),
            });
        }
    }
    if let Some(stat) = git_stdout(repo_root, &["diff", "--stat"]) {
        let stat = stat.trim();
        if !stat.is_empty() {
            out.push(Fragment {
                text: stat.to_string(),
                source_path: "git:diff".to_string(),
                authority: Authority::Mid,
                score: stat.lines().count() as i64,
                anchor: Some("git diff --stat".to_string()),
            });
        }
    }
}

/// taskprog progress (DESIGN §3): `.claude/progress.md` preferred, else
/// `progress.md`. Mid authority. Missing file => no fragment.
fn gather_progress(out: &mut Vec<Fragment>, repo_root: &Path) {
    for rel in [".claude/progress.md", "progress.md"] {
        let path = repo_root.join(rel);
        if let Ok(text) = std::fs::read_to_string(&path) {
            let text = text.trim();
            if !text.is_empty() {
                out.push(Fragment {
                    text: text.to_string(),
                    source_path: rel.to_string(),
                    authority: Authority::Mid,
                    score: text.lines().count() as i64,
                    anchor: Some("taskprog".to_string()),
                });
            }
            return; // first found wins; don't double-count both files.
        }
    }
}

/// deepwiki (DESIGN §3): any `.deepwiki/**/*.md`. Mid authority. Missing dir =>
/// no fragments. Walks deterministically (sorted) so the bundle is stable.
fn gather_deepwiki(out: &mut Vec<Fragment>, repo_root: &Path) {
    let root = repo_root.join(".deepwiki");
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    collect_md(&root, &mut files);
    files.sort();
    for path in files {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            let rel = path
                .strip_prefix(repo_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push(Fragment {
                text: text.to_string(),
                source_path: rel,
                authority: Authority::Mid,
                score: text.lines().count() as i64,
                anchor: Some("deepwiki".to_string()),
            });
        }
    }
}

/// Recursively collect `*.md` files under `dir` (deterministic; tolerant of a
/// missing directory).
fn collect_md(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

/// Run `git <args>` in `repo_root`, returning trimmed stdout on success.
/// Returns `None` for any failure (non-git dir, missing git, non-zero exit) so
/// callers can treat git as an optional source.
fn git_stdout(repo_root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gather_tolerates_non_git_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let charter = Charter::default();
        let cfg = Config::default();
        // No git, no progress, no deepwiki: must not error, must be empty.
        let bundle = gather(dir.path(), &charter, &cfg).expect("gather must not error");
        assert!(bundle.fragments.is_empty());
    }

    #[test]
    fn gather_includes_charter_and_progress() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::write(dir.path().join(".claude/progress.md"), "- did a thing\n").unwrap();

        let charter = Charter {
            north_star: "Ship compass.".to_string(),
            definition_of_done: vec!["crate builds".to_string()],
            next_action: "write gather.rs".to_string(),
            ..Charter::default()
        };
        let bundle = gather(dir.path(), &charter, &Config::default()).expect("gather");

        assert!(bundle
            .fragments
            .iter()
            .any(|f| f.anchor.as_deref() == Some("north_star") && f.authority == Authority::High));
        assert!(bundle
            .fragments
            .iter()
            .any(|f| f.anchor.as_deref() == Some("taskprog")));
    }
}
