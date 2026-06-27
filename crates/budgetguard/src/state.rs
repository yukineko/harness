//! Persistent daily spend ledger.
//!
//! `~/.budgetguard/state/ledger.json` maps `YYYY-MM-DD` →
//! `{ sessions: { <id>: <cost_usd> } }`. On each Stop we overwrite the current
//! session's entry and recompute the day total, so the file is always consistent
//! with the latest transcript. Old days accumulate naturally (prune if desired).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DayEntry {
    /// session_id → latest cost for that session on this day.
    pub sessions: BTreeMap<String, f64>,
}

impl DayEntry {
    pub fn total(&self) -> f64 {
        self.sessions.values().sum()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Ledger {
    pub days: BTreeMap<String, DayEntry>,
}

/// Why a ledger couldn't be loaded as a clean, current value.
#[derive(Debug)]
pub enum LoadError {
    /// The file exists but isn't valid JSON for a `Ledger`. The on-disk bytes
    /// must be PRESERVED (never overwritten with a reset/default), or one bad
    /// write would erase the whole spend history and fail the budget open.
    Corrupt,
}

impl Ledger {
    fn path(state_dir: &Path) -> PathBuf {
        state_dir.join("ledger.json")
    }

    /// Load the ledger, distinguishing "absent" (a fresh default is correct)
    /// from "present but corrupt" (the caller must not clobber it).
    ///
    /// - file missing            → `Ok(Ledger::default())`
    /// - file present & parses    → `Ok(parsed)`
    /// - file present & unparseable → `Err(LoadError::Corrupt)`
    pub fn load_checked(state_dir: &Path) -> Result<Self, LoadError> {
        match std::fs::read_to_string(Self::path(state_dir)) {
            Err(_) => Ok(Self::default()), // absent (or unreadable) → fresh
            Ok(s) => serde_json::from_str(&s).map_err(|_| LoadError::Corrupt),
        }
    }

    /// Backwards-compatible lenient load: a corrupt file reads as default.
    /// Prefer [`Ledger::load_checked`] where clobbering must be avoided.
    pub fn load(state_dir: &Path) -> Self {
        Self::load_checked(state_dir).unwrap_or_default()
    }

    /// Persist the ledger atomically (tmp file + rename) so a concurrent reader
    /// — or a crash mid-write — never observes a truncated/partial JSON file
    /// that would then parse-fail and be treated as corrupt.
    pub fn save(&self, state_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(state_dir)?;
        let json = serde_json::to_string_pretty(self).unwrap_or_default();
        let final_path = Self::path(state_dir);
        let tmp_path = final_path.with_extension("json.tmp");
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &final_path)
    }

    /// Update the session entry for `today`, return the updated day total.
    pub fn record(&mut self, session_id: &str, today: &str, cost: f64) -> f64 {
        let entry = self.days.entry(today.to_string()).or_default();
        entry.sessions.insert(session_id.to_string(), cost);
        entry.total()
    }

    pub fn day_total(&self, date: &str) -> f64 {
        self.days.get(date).map(|e| e.total()).unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A throwaway temp dir (no external crate); removed on drop.
    struct TmpDir(PathBuf);
    impl TmpDir {
        fn new(tag: &str) -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let p = std::env::temp_dir()
                .join(format!("budgetguard-{tag}-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&p).unwrap();
            TmpDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn record_sums_sessions_into_day_total() {
        let mut l = Ledger::default();
        assert_eq!(l.record("s1", "2026-06-27", 1.5), 1.5);
        // A different session adds to the same day.
        assert_eq!(l.record("s2", "2026-06-27", 2.0), 3.5);
        // Re-recording the SAME session overwrites (not adds) — latest cost wins.
        assert_eq!(l.record("s1", "2026-06-27", 4.0), 6.0);
        assert_eq!(l.day_total("2026-06-27"), 6.0);
        assert_eq!(l.day_total("2026-06-26"), 0.0);
    }

    #[test]
    fn save_then_load_roundtrips() {
        let d = TmpDir::new("roundtrip");
        let mut l = Ledger::default();
        l.record("s1", "2026-06-27", 3.25);
        l.save(d.path()).unwrap();
        let loaded = Ledger::load_checked(d.path()).unwrap();
        assert_eq!(loaded.day_total("2026-06-27"), 3.25);
    }

    #[test]
    fn absent_file_loads_as_default_not_error() {
        let d = TmpDir::new("absent");
        // Nothing written yet → Ok(default), not Err.
        let l = Ledger::load_checked(d.path()).unwrap();
        assert_eq!(l.day_total("2026-06-27"), 0.0);
    }

    #[test]
    fn corrupt_file_is_an_error_and_is_preserved() {
        let d = TmpDir::new("corrupt");
        let p = Ledger::path(d.path());
        std::fs::write(&p, b"{ this is not valid json").unwrap();

        // load_checked surfaces corruption rather than silently resetting.
        assert!(matches!(
            Ledger::load_checked(d.path()),
            Err(LoadError::Corrupt)
        ));
        // The caller (gate) must not have overwritten it — verify the bytes
        // are still the corrupt original (preserved, not reset to "{}").
        let after = std::fs::read_to_string(&p).unwrap();
        assert_eq!(after, "{ this is not valid json");

        // The lenient load() still degrades to default for back-compat.
        assert_eq!(Ledger::load(d.path()).day_total("x"), 0.0);
    }

    #[test]
    fn save_leaves_no_tmp_file_behind() {
        let d = TmpDir::new("notmp");
        Ledger::default().save(d.path()).unwrap();
        let tmp = Ledger::path(d.path()).with_extension("json.tmp");
        assert!(!tmp.exists(), "atomic save must rename, not leave a .tmp");
        assert!(Ledger::path(d.path()).exists());
    }
}
