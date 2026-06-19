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
}
