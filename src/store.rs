//! Durable note store. Notes are Obsidian-compatible markdown, grouped per
//! project (keyed by cwd). The store dir can point at a real Obsidian vault.

use std::path::{Path, PathBuf};

use crate::config::Config;

/// Stable, human-readable project key from a cwd: basename + short hash of the
/// full path (so two different `src/` dirs don't collide).
pub fn project_key(cwd: &Path) -> String {
    let base = cwd
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project");
    let safe: String = base
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let h = short_hash(&cwd.to_string_lossy());
    format!("{safe}-{h}")
}

/// FNV-1a 32-bit, hex. Small, dependency-free, stable across runs.
fn short_hash(s: &str) -> String {
    let mut hash: u32 = 0x811c9dc5;
    for b in s.bytes() {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("{hash:08x}")
}

/// Short, stable tag for a session id, embedded in note filenames so a session
/// can deterministically find its own notes even when sibling sessions write
/// into the same project dir in parallel. Empty id → "nosess".
pub fn session_tag(session_id: &str) -> String {
    if session_id.is_empty() {
        "nosess".to_string()
    } else {
        short_hash(session_id)
    }
}

pub struct Store {
    pub root: PathBuf,
}

impl Store {
    pub fn new(cfg: &Config) -> Self {
        Store {
            root: cfg.store_dir.clone(),
        }
    }

    /// Directory holding a project's notes (created on demand by `write`).
    pub fn project_dir(&self, cwd: &Path) -> PathBuf {
        self.root.join(project_key(cwd))
    }

    /// Write a note. `slug` is a filesystem-safe stem; returns the full path.
    pub fn write_note(&self, cwd: &Path, slug: &str, body: &str) -> std::io::Result<PathBuf> {
        let dir = self.project_dir(cwd);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{slug}.md"));
        std::fs::write(&path, body)?;
        Ok(path)
    }

    /// All `.md` notes in a project's dir, newest first (by modified time).
    pub fn list_notes(&self, cwd: &Path) -> Vec<PathBuf> {
        let dir = self.project_dir(cwd);
        let mut entries: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|s| s.to_str()) == Some("md") {
                    let mtime = e
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::UNIX_EPOCH);
                    entries.push((mtime, p));
                }
            }
        }
        entries.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
        entries.into_iter().map(|(_, p)| p).collect()
    }

    /// Most recent note for a project, if any.
    pub fn latest_note(&self, cwd: &Path) -> Option<PathBuf> {
        self.list_notes(cwd).into_iter().next()
    }

    #[cfg(test)]
    pub fn write_note_named(&self, cwd: &Path, name: &str, body: &str) -> std::io::Result<PathBuf> {
        let dir = self.project_dir(cwd);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{name}.md"));
        std::fs::write(&path, body)?;
        Ok(path)
    }

    /// Most recent note whose filename carries this session's tag. Lets the
    /// originating session reach its own note amid parallel sessions sharing the
    /// project dir. None if the id is empty or no tagged note exists.
    pub fn latest_note_for_session(&self, cwd: &Path, session_id: &str) -> Option<PathBuf> {
        if session_id.is_empty() {
            return None;
        }
        let needle = format!("-{}-", session_tag(session_id));
        self.list_notes(cwd).into_iter().find(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .map(|n| n.contains(&needle))
                .unwrap_or(false)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn temp_store(name: &str) -> (Config, PathBuf) {
        let root = std::env::temp_dir().join(format!("ctxrot-test-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut cfg = Config::default();
        cfg.store_dir = root.clone();
        (cfg, root)
    }

    #[test]
    fn session_tag_is_stable_and_distinct() {
        assert_eq!(session_tag("sess-A"), session_tag("sess-A"));
        assert_ne!(session_tag("sess-A"), session_tag("sess-B"));
        assert_eq!(session_tag(""), "nosess");
    }

    #[test]
    fn session_routing_picks_own_note() {
        let (cfg, root) = temp_store("routing");
        let store = Store::new(&cfg);
        let cwd = Path::new("/some/project");
        let a = session_tag("session-A");
        let b = session_tag("session-B");

        store.write_note_named(cwd, &format!("distill-{a}-20260619-100000"), "mine").unwrap();
        store.write_note_named(cwd, &format!("rescue-{b}-20260619-110000"), "theirs").unwrap();

        let mine = store.latest_note_for_session(cwd, "session-A").unwrap();
        assert!(mine.to_string_lossy().contains(&a));
        assert!(!mine.to_string_lossy().contains(&b));

        // Unknown session → no tagged match, caller falls back to latest_note.
        assert!(store.latest_note_for_session(cwd, "session-C").is_none());
        // Empty session id is never routed.
        assert!(store.latest_note_for_session(cwd, "").is_none());

        let _ = std::fs::remove_dir_all(&root);
    }
}
