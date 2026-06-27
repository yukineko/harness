//! Per-project addressing for run-state files — the SINGLE source of truth.
//!
//! A project key is `<sanitized-basename>-<fnv1a32-hex-of-canonical-root>`: the
//! basename keeps it readable, the hash keeps two same-named repos from
//! colliding. FNV-1a is dependency-free and stable across runs.
//!
//! This lives in harness-core because more than one plugin must derive the SAME
//! key for the same repo: condukt writes its run-state under `project_key(root)`
//! and autoflow reads that very directory. When each crate kept its own private
//! copy, a change in one would silently send the other to a different directory
//! (losing state). Both now call this; they cannot drift.
//!
//! NOTE: this is intentionally distinct from `store::project_key`, which keys the
//! per-cwd note store with a different (alnum-only, non-canonicalized) scheme.

use std::path::{Path, PathBuf};

/// FNV-1a 32-bit hash. Small, dependency-free, stable across runs.
pub fn fnv1a32(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// Stable per-project key derived from the canonical repo root.
pub fn project_key(root: &Path) -> String {
    let canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let full = canon.to_string_lossy();
    let base = canon
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "root".into());
    let sani: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("{}-{:08x}", sani, fnv1a32(&full))
}

/// Nearest ancestor containing `.git`; falls back to `cwd` if none.
pub fn repo_root(cwd: &Path) -> PathBuf {
    let mut cur = cwd.to_path_buf();
    loop {
        if cur.join(".git").exists() {
            return cur;
        }
        if !cur.pop() {
            break;
        }
    }
    cwd.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_stable_and_sanitized() {
        let p = PathBuf::from("/tmp/My Repo");
        let k1 = project_key(&p);
        let k2 = project_key(&p);
        assert_eq!(k1, k2);
        // basename sanitized (space -> '-'), hash suffix present.
        assert!(k1.starts_with("My-Repo-"));
        assert_eq!(k1.len(), "My-Repo-".len() + 8);
    }

    #[test]
    fn distinct_paths_get_distinct_hashes() {
        let a = project_key(&PathBuf::from("/tmp/proj"));
        let b = project_key(&PathBuf::from("/var/proj"));
        assert_ne!(a, b);
    }

    #[test]
    fn fnv_known_vector() {
        // FNV-1a 32-bit of empty string is the offset basis.
        assert_eq!(fnv1a32(""), 0x811c_9dc5);
        // A second fixed vector pins the multiply step so the algorithm can't
        // silently change (which would relocate every project's state dir).
        assert_eq!(fnv1a32("a"), 0xe40c_292c);
    }

    #[test]
    fn key_format_is_basename_dash_hash() {
        // Guards the on-disk layout: <sanitized-basename>-<8 hex>. A non-existent
        // path can't be canonicalized, so the input path is used as-is.
        let p = PathBuf::from("/tmp/proj_x");
        let expected = format!("proj_x-{:08x}", fnv1a32(&p.to_string_lossy()));
        assert_eq!(project_key(&p), expected);
    }
}
