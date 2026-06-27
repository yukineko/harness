//! Decision records (ADRs): the *why* behind a canon change, pinned to the
//! canon commit it was made against.
//!
//! The harness only *lists* and *scaffolds* these notes — it never parses their
//! semantics. The D3 audit (see the decisions prompt) makes the agent read each
//! record's live content, extract its canon pins / drivers / review-when, and
//! check them against the live canon. This keeps the one-truth discipline: the
//! record is evidence pinned to a commit, never a second authority.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Resolve the decisions directory. Empty `dir` disables the feature (None).
/// Absolute paths (e.g. an Obsidian vault) are used as-is; relative paths
/// resolve under `repo_root`.
pub fn resolve_dir(repo_root: &Path, dir: &str) -> Option<PathBuf> {
    let dir = dir.trim();
    if dir.is_empty() {
        return None;
    }
    let p = Path::new(dir);
    Some(if p.is_absolute() {
        p.to_path_buf()
    } else {
        repo_root.join(p)
    })
}

/// List decision record files (`*.md`) under the decisions dir, sorted. Returns
/// absolute path strings the read-only agent can open. Empty if the feature is
/// disabled or the dir is missing/unreadable.
pub fn list_files(repo_root: &Path, dir: &str) -> Vec<String> {
    let Some(d) = resolve_dir(repo_root, dir) else {
        return vec![];
    };
    let Ok(entries) = std::fs::read_dir(&d) else {
        return vec![];
    };
    let mut files: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    files.sort();
    files
}

/// Slugify a title for the record id. Unicode-aware so Japanese titles keep
/// their characters; runs of non-alphanumerics collapse to a single dash.
pub fn slug(title: &str) -> String {
    let mut s = String::new();
    let mut prev_dash = false;
    for ch in title.chars() {
        if ch.is_alphanumeric() {
            for lc in ch.to_lowercase() {
                s.push(lc);
            }
            prev_dash = false;
        } else if !prev_dash && !s.is_empty() {
            s.push('-');
            prev_dash = true;
        }
    }
    while s.ends_with('-') {
        s.pop();
    }
    if s.is_empty() {
        "decision".to_string()
    } else {
        s
    }
}

/// Write a starter decision record pinned to `canon_commit`. Returns the path.
/// Errors (without overwriting) if it already exists and `force` is false.
pub fn scaffold(
    repo_root: &Path,
    dir: &str,
    id: &str,
    title: &str,
    date: &str,
    canon_commit: &str,
    force: bool,
) -> Result<PathBuf> {
    let d = resolve_dir(repo_root, dir).unwrap_or_else(|| repo_root.join("decisions"));
    std::fs::create_dir_all(&d).with_context(|| format!("creating {}", d.display()))?;
    let path = d.join(format!("{id}.md"));
    if path.exists() && !force {
        anyhow::bail!(
            "{} already exists (use --force to overwrite)",
            path.display()
        );
    }
    let body = format!(
        "---\n\
         id: {id}\n\
         title: \"{title}\"\n\
         date: {date}\n\
         status: proposed\n\
         canon_commit: {canon_commit}\n\
         canon: []          # この決定が支配する canon ポインタ (file または file:section)\n\
         drivers: []        # 反証可能な理由 (例: \"HMAC 鍵ローテーションが単一署名経路を要求\")\n\
         review_when: \"\"    # この条件が成立したら再検討 (driver が崩れる条件)\n\
         supersedes: []     # 置き換えた決定があれば (例: [[2026-01-01-old]])\n\
         ---\n\n\
         ## 決定\n\n\
         (何を変えたか。中身を写さず canon は `canon:` にポインタで。)\n\n\
         ## 理由 (なぜ)\n\n\
         (反証可能な driver と、却下した代替を書く。`drivers:` / `review_when:` を埋める。)\n\n\
         ## 影響\n\n\
         (関連する実装・canon。`[[他の決定]]` でリンク可。)\n"
    );
    std::fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_ascii_and_collapse() {
        assert_eq!(slug("Single Signing Path!"), "single-signing-path");
        assert_eq!(slug("  a -- b  "), "a-b");
        assert_eq!(slug("!!!"), "decision");
    }

    #[test]
    fn slug_keeps_unicode() {
        assert_eq!(slug("署名の単一経路"), "署名の単一経路");
    }
}
