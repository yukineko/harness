//! The per-project "loadset": the user's explicit choices about what context to
//! keep around (`pinned`) and what to keep out (`dropped`), persisted so they
//! survive across sessions. Backs the `/ctx` skill and `ctxrot ctx` subcommands.
//!
//! Why a separate store from notes: notes are *content* (carryover text); the
//! loadset is *policy* (a small list of paths/labels). It lives at
//! `<state_dir>/loadset-<project_key>.json`, one file per project, so parallel
//! sessions in the same project share one intent set (last writer wins — these
//! are deliberate human edits, not high-frequency machine writes).
//!
//! Cardinal rule still holds: every read is best-effort. A missing/corrupt file
//! yields an empty loadset, never an error that could break a hook turn.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use harness_core::store::project_key;

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoadSet {
    /// Paths/labels the user wants re-surfaced (as a pointer, never inlined) at
    /// session start. Order preserved; newest appended last.
    #[serde(default)]
    pub pinned: Vec<String>,
    /// Paths/labels the user wants kept OUT of context. Hooks can't evict live
    /// tokens, so this is advisory: honored by the next compaction / distill /
    /// fresh-session carryover, and surfaced by `/ctx` for a manual `/compact`.
    #[serde(default)]
    pub dropped: Vec<String>,
    /// When set, `restore` uses this specific note path instead of auto-selecting
    /// the latest. Set with `ctxrot ctx use-note <path>`, clear with
    /// `ctxrot ctx clear-note`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_note: Option<String>,
}

/// The on-disk path for a project's loadset.
pub fn path_for(state_dir: &Path, cwd: &Path) -> PathBuf {
    state_dir.join(format!("loadset-{}.json", project_key(cwd)))
}

impl LoadSet {
    /// Load the loadset for `cwd`, or an empty one if absent/unreadable/corrupt.
    pub fn load(state_dir: &Path, cwd: &Path) -> Self {
        let p = path_for(state_dir, cwd);
        std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Persist atomically-ish (write whole file). Creates `state_dir` if needed.
    pub fn save(&self, state_dir: &Path, cwd: &Path) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(state_dir)?;
        let p = path_for(state_dir, cwd);
        let body = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into());
        std::fs::write(&p, body)?;
        Ok(p)
    }

    /// Add `item` to `pinned` (de-duped, and removed from `dropped` if present).
    /// Returns true if anything changed.
    pub fn pin(&mut self, item: &str) -> bool {
        let was_dropped = remove_item(&mut self.dropped, item);
        if self.pinned.iter().any(|x| x == item) {
            return was_dropped;
        }
        self.pinned.push(item.to_string());
        true
    }

    /// Remove `item` from `pinned`. Returns true if it was present.
    pub fn unpin(&mut self, item: &str) -> bool {
        remove_item(&mut self.pinned, item)
    }

    /// Add `item` to `dropped` (de-duped, and removed from `pinned` if present).
    /// Returns true if anything changed.
    pub fn drop_item(&mut self, item: &str) -> bool {
        let was_pinned = remove_item(&mut self.pinned, item);
        if self.dropped.iter().any(|x| x == item) {
            return was_pinned;
        }
        self.dropped.push(item.to_string());
        true
    }

    /// Remove `item` from `dropped`. Returns true if it was present.
    pub fn undrop(&mut self, item: &str) -> bool {
        remove_item(&mut self.dropped, item)
    }

    /// Set the preferred note path for `restore` to use instead of auto-selection.
    /// Returns true if anything changed.
    pub fn set_preferred_note(&mut self, path: &str) -> bool {
        if self.preferred_note.as_deref() == Some(path) {
            return false;
        }
        self.preferred_note = Some(path.to_string());
        true
    }

    /// Clear the preferred note, reverting `restore` to auto-selection.
    /// Returns true if anything changed.
    pub fn clear_preferred_note(&mut self) -> bool {
        if self.preferred_note.is_none() {
            return false;
        }
        self.preferred_note = None;
        true
    }

    pub fn is_empty(&self) -> bool {
        self.pinned.is_empty() && self.dropped.is_empty() && self.preferred_note.is_none()
    }
}

fn remove_item(v: &mut Vec<String>, item: &str) -> bool {
    let before = v.len();
    v.retain(|x| x != item);
    v.len() != before
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("ctxrot-loadset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn pin_and_drop_are_mutually_exclusive() {
        let mut ls = LoadSet::default();
        assert!(ls.pin("a.md"));
        assert!(ls.drop_item("a.md")); // moving a→dropped changes both lists
        assert_eq!(ls.pinned, Vec::<String>::new());
        assert_eq!(ls.dropped, vec!["a.md".to_string()]);
        assert!(ls.pin("a.md")); // moving back
        assert_eq!(ls.pinned, vec!["a.md".to_string()]);
        assert!(ls.dropped.is_empty());
    }

    #[test]
    fn pin_dedupes() {
        let mut ls = LoadSet::default();
        assert!(ls.pin("a"));
        assert!(!ls.pin("a")); // no change second time
        assert_eq!(ls.pinned.len(), 1);
    }

    #[test]
    fn unpin_and_undrop() {
        let mut ls = LoadSet::default();
        ls.pin("a");
        assert!(ls.unpin("a"));
        assert!(!ls.unpin("a"));
        ls.drop_item("b");
        assert!(ls.undrop("b"));
        assert!(!ls.undrop("b"));
        assert!(ls.is_empty());
    }

    #[test]
    fn roundtrips_through_disk() {
        let dir = tmp();
        let cwd = dir.join("proj");
        std::fs::create_dir_all(&cwd).unwrap();
        let mut ls = LoadSet::default();
        ls.pin("docs/spec.md");
        ls.drop_item("huge.log");
        let saved = ls.save(&dir, &cwd).unwrap();
        assert!(saved.exists());
        let back = LoadSet::load(&dir, &cwd);
        assert_eq!(ls, back);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_empty_not_error() {
        let dir = tmp();
        let cwd = dir.join("nope");
        let ls = LoadSet::load(&dir, &cwd);
        assert!(ls.is_empty());
    }
}
