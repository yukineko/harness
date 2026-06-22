//! Per-project addressing for run-state files.
//!
//! A project key is `<sanitized-basename>-<fnv1a32-hex-of-canonical-root>`: the
//! basename keeps it readable, the hash keeps two same-named repos from
//! colliding. FNV-1a is dependency-free and stable across runs.

use std::path::{Path, PathBuf};

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
    use std::path::PathBuf;

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
    }
}
