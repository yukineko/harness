//! Durable note store. Notes are Obsidian-compatible markdown, grouped per
//! project (keyed by cwd). The store dir can point at a real Obsidian vault.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use regex::Regex;

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

/// The session tag embedded in a note filename, if it follows the tagged scheme
/// `<slug>-<tag>-<YYYYMMDD>-<HHMMSS>` (tag = 8 hex from `short_hash`, or `nosess`).
/// Returns None for legacy/untagged notes — the signal `latest_fallback_note`
/// uses to tell streams apart.
fn note_session_tag(path: &Path) -> Option<String> {
    let stem = path.file_stem().and_then(|s| s.to_str())?;
    tagged_note_re()
        .captures(stem)
        .map(|c| c[1].to_string())
}

fn tagged_note_re() -> Regex {
    Regex::new(r"-([0-9a-f]{8}|nosess)-\d{8}-\d{6}$").expect("static regex")
}

/// A `distill-*` note (the high-value, LLM-distilled carryover), as opposed to a
/// deterministic `rescue-*`. Used by `prune` to protect distills preferentially,
/// and by `restore` to nudge when only deterministic rescues exist.
pub fn is_distill(path: &Path) -> bool {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(|n| n.starts_with("distill-"))
        .unwrap_or(false)
}

/// Outcome of `Store::prune`: how many notes survived and which were removed
/// (the removal set is also the dry-run preview).
pub struct PruneResult {
    pub kept: usize,
    pub removed: Vec<PathBuf>,
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

    /// Cross-session fallback note for `restore` when this session has no note of
    /// its own. Prevents grabbing a *sibling* stream's carryover in shared-cwd
    /// parallel use, WITHOUT breaking ordinary cross-session continuity:
    ///   * ≤1 distinct session tag in the dir → unambiguous (single stream, or a
    ///     prior sequential session) → return the latest note of any kind.
    ///   * ≥2 distinct session tags → parallel usage detected → restrict to
    ///     untagged (legacy / explicitly-shared) notes; never another session's.
    ///
    /// (Own-session notes are already handled by `latest_note_for_session`, so by
    /// the time we get here the tags present belong to *other* sessions.)
    pub fn latest_fallback_note(&self, cwd: &Path) -> Option<PathBuf> {
        let notes = self.list_notes(cwd);
        let distinct: HashSet<String> = notes.iter().filter_map(|p| note_session_tag(p)).collect();
        if distinct.len() <= 1 {
            notes.into_iter().next()
        } else {
            notes.into_iter().find(|p| note_session_tag(p).is_none())
        }
    }

    /// Most recent `rescue-<tag>-*` note for this session whose mtime is within
    /// `within_secs` of now — the coalescing probe (P3). None when there's no such
    /// fresh rescue, so the caller writes a new one.
    pub fn recent_session_rescue(
        &self,
        cwd: &Path,
        session_id: &str,
        within_secs: u64,
    ) -> Option<PathBuf> {
        if session_id.is_empty() {
            return None;
        }
        let prefix = format!("rescue-{}-", session_tag(session_id));
        let cutoff = std::time::SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(within_secs))
            .unwrap_or(std::time::UNIX_EPOCH);
        // list_notes is newest-first, so the first match in window wins.
        for p in self.list_notes(cwd) {
            let is_ours = p
                .file_name()
                .and_then(|s| s.to_str())
                .map(|n| n.starts_with(&prefix))
                .unwrap_or(false);
            if !is_ours {
                continue;
            }
            let fresh = std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .map(|t| t >= cutoff)
                .unwrap_or(false);
            if fresh {
                return Some(p);
            }
        }
        None
    }

    /// GC (`ctxrot note prune`): keep the newest `keep` notes overall, plus the
    /// newest `keep_distill_min` `distill-*` notes (higher value than rescues)
    /// even if they fall outside that window; delete the rest. `dry_run` computes
    /// the removal set without touching disk. Deletes are best-effort.
    pub fn prune(
        &self,
        cwd: &Path,
        keep: usize,
        keep_distill_min: usize,
        dry_run: bool,
    ) -> PruneResult {
        let notes = self.list_notes(cwd); // newest first
        let mut protect: HashSet<PathBuf> = notes.iter().take(keep).cloned().collect();
        for p in notes.iter().filter(|p| is_distill(p)).take(keep_distill_min) {
            protect.insert(p.clone());
        }
        let mut removed = Vec::new();
        for p in &notes {
            if protect.contains(p) {
                continue;
            }
            if !dry_run {
                let _ = std::fs::remove_file(p);
            }
            removed.push(p.clone());
        }
        PruneResult {
            kept: notes.len() - removed.len(),
            removed,
        }
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
        let cfg = Config {
            store_dir: root.clone(),
            ..Config::default()
        };
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

    #[test]
    fn detects_filename_session_tag() {
        let a = session_tag("session-A");
        assert_eq!(
            note_session_tag(Path::new(&format!("/x/distill-{a}-20260619-100000.md"))),
            Some(a)
        );
        assert_eq!(
            note_session_tag(Path::new("/x/rescue-nosess-20260619-100000.md")),
            Some("nosess".to_string())
        );
        // Legacy/untagged notes carry no session tag.
        assert_eq!(note_session_tag(Path::new("/x/rescue-20260619-100000.md")), None);
        assert_eq!(note_session_tag(Path::new("/x/handwritten-notes.md")), None);
    }

    #[test]
    fn fallback_single_stream_keeps_continuity() {
        let (cfg, root) = temp_store("fb-single");
        let store = Store::new(&cfg);
        let cwd = Path::new("/some/project");
        let a = session_tag("prev-session");

        // Only one (prior, sequential) session's notes → unambiguous → return latest.
        store.write_note_named(cwd, &format!("distill-{a}-20260619-100000"), "old").unwrap();
        store.write_note_named(cwd, &format!("rescue-{a}-20260619-110000"), "newer").unwrap();
        let fb = store.latest_fallback_note(cwd).unwrap();
        assert!(std::fs::read_to_string(&fb).unwrap().contains("newer"));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Write `n` notes with the given slug prefix, oldest first, nudging mtime
    /// forward so `list_notes` ordering is deterministic.
    fn write_series(store: &Store, cwd: &Path, prefix: &str, n: usize) {
        for i in 0..n {
            store
                .write_note_named(cwd, &format!("{prefix}-{i:02}"), &format!("body {i}"))
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn prune_dry_run_removes_nothing() {
        let (cfg, root) = temp_store("prune-dry");
        let store = Store::new(&cfg);
        let cwd = Path::new("/some/project");
        write_series(&store, cwd, "rescue-aaaaaaaa-2026010", 5);

        let res = store.prune(cwd, 2, 0, true);
        assert_eq!(res.removed.len(), 3);
        // Nothing actually deleted.
        assert_eq!(store.list_notes(cwd).len(), 5);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_keeps_newest_n() {
        let (cfg, root) = temp_store("prune-n");
        let store = Store::new(&cfg);
        let cwd = Path::new("/some/project");
        write_series(&store, cwd, "rescue-aaaaaaaa-2026010", 5);

        let res = store.prune(cwd, 2, 0, false);
        assert_eq!(res.removed.len(), 3);
        assert_eq!(store.list_notes(cwd).len(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_protects_distill_floor() {
        let (cfg, root) = temp_store("prune-distill");
        let store = Store::new(&cfg);
        let cwd = Path::new("/some/project");
        // Oldest = one distill, then 4 rescues on top.
        store.write_note_named(cwd, "distill-aaaaaaaa-20260101-000000", "d").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        write_series(&store, cwd, "rescue-aaaaaaaa-2026010", 4);

        // keep newest 2 (both rescues) + protect newest 1 distill (the old one).
        let res = store.prune(cwd, 2, 1, false);
        assert_eq!(res.removed.len(), 2); // the two oldest rescues
        let remaining = store.list_notes(cwd);
        assert_eq!(remaining.len(), 3);
        assert!(
            remaining.iter().any(|p| is_distill(p)),
            "the distill note must survive: {remaining:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn fallback_parallel_avoids_sibling_tagged_note() {
        let (cfg, root) = temp_store("fb-parallel");
        let store = Store::new(&cfg);
        let cwd = Path::new("/some/project");
        let a = session_tag("sib-A");
        let b = session_tag("sib-B");

        // Two distinct sessions → parallel → must NOT return either tagged note.
        store.write_note_named(cwd, &format!("distill-{a}-20260619-100000"), "A").unwrap();
        store.write_note_named(cwd, &format!("rescue-{b}-20260619-110000"), "B").unwrap();
        assert!(store.latest_fallback_note(cwd).is_none());

        // With an untagged shared note present, fall back to that instead.
        store.write_note_named(cwd, "shared-handoff", "shared").unwrap();
        let fb = store.latest_fallback_note(cwd).unwrap();
        assert!(std::fs::read_to_string(&fb).unwrap().contains("shared"));

        let _ = std::fs::remove_dir_all(&root);
    }
}
