//! Deterministic fingerprint of the SKILL.md corpus under a directory.
//!
//! A silent edit to a `SKILL.md` shifts agent behaviour without changing any
//! recorded field, so outcomes drift away from the skill version that produced
//! them. Stamping each `Episode` with this fingerprint lets us stratify outcomes
//! by skill version after the fact. std-only (DefaultHasher), mirroring the id
//! hashing tracekit uses — no external crate just for this.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

/// Walk `root` recursively for files named exactly `SKILL.md`, hash their
/// sorted (relative-path, content) pairs, and return a short lowercase hex
/// string. Deterministic: the same corpus always yields the same hex, and any
/// changed/added/removed SKILL.md changes it. Unreadable files are skipped
/// rather than aborting the whole walk (fail-soft, like the store).
pub fn skill_fingerprint(root: &Path) -> std::io::Result<String> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    collect_skills(root, root, &mut pairs);
    // Sort by relative path so directory-iteration order can't perturb the hash.
    pairs.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = DefaultHasher::new();
    for (rel, content) in &pairs {
        // Hash path and content as distinct, length-delimited fields (Hash for
        // str already feeds a length) so "ab"+"c" can't collide with "a"+"bc".
        rel.hash(&mut hasher);
        content.hash(&mut hasher);
    }
    let hash = hasher.finish();
    Ok(format!("{hash:016x}"))
}

/// Recursively gather (relative-path-from-`base`, content) for every `SKILL.md`.
/// Errors on a single entry (unreadable dir/file) are swallowed so one bad path
/// never sinks the whole fingerprint.
fn collect_skills(base: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_skills(base, &path, out);
        } else if file_type.is_file() && path.file_name() == Some(std::ffi::OsStr::new("SKILL.md"))
        {
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue; // skip unreadable / non-UTF8 file, keep walking
            };
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            out.push((rel, content));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A throwaway temp dir under the system temp, keyed by pid + tag so parallel
    /// tests don't collide. Returns the created path.
    fn tmp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("fugu-fingerprint-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn stable_across_calls_same_tree() {
        let dir = tmp_dir("stable");
        write(&dir.join("a/SKILL.md"), "alpha skill");
        write(&dir.join("b/SKILL.md"), "beta skill");
        // a non-SKILL file must not affect the fingerprint
        write(&dir.join("a/README.md"), "ignored");

        let first = skill_fingerprint(&dir).unwrap();
        let second = skill_fingerprint(&dir).unwrap();
        assert_eq!(first, second, "same corpus must hash identically");
        assert!(!first.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn changes_when_skill_content_changes() {
        let dir = tmp_dir("changed");
        let skill = dir.join("a/SKILL.md");
        write(&skill, "original guidance");
        let before = skill_fingerprint(&dir).unwrap();

        write(&skill, "edited guidance");
        let after = skill_fingerprint(&dir).unwrap();
        assert_ne!(
            before, after,
            "an edited SKILL.md must change the fingerprint"
        );

        // adding another SKILL.md must also change it
        write(&dir.join("c/SKILL.md"), "new skill");
        let with_added = skill_fingerprint(&dir).unwrap();
        assert_ne!(
            after, with_added,
            "an added SKILL.md must change the fingerprint"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_or_missing_tree_is_stable() {
        let dir = tmp_dir("empty");
        let a = skill_fingerprint(&dir).unwrap();
        let b = skill_fingerprint(&dir).unwrap();
        assert_eq!(a, b);
        // a missing root reads as an empty corpus rather than erroring
        let missing = dir.join("does-not-exist");
        assert_eq!(skill_fingerprint(&missing).unwrap(), a);
        std::fs::remove_dir_all(&dir).ok();
    }
}
