//! Guard that allows an action to run at most once per calendar day.
//!
//! State is persisted as `<state_dir>/<key>-daily.txt` containing a single
//! `YYYY-MM-DD` line. If the stored date matches today, the action is skipped.
//! Any other content (missing file, different date, empty) triggers a run.
//!
//! Callers are responsible for providing `today` so this module stays
//! dependency-free from chrono:
//! ```ignore
//! let today = chrono::Local::now().format("%Y-%m-%d").to_string();
//! let guard = DailyGuard::new(state_dir, "my-plugin", &today);
//! if guard.should_run() {
//!     do_daily_work();
//!     guard.mark_done().ok();
//! }
//! ```

use std::path::{Path, PathBuf};

/// Guard that allows an action to run at most once per calendar day.
pub struct DailyGuard {
    path: PathBuf,
    today: String,
}

impl DailyGuard {
    /// Create a guard keyed by `key`, using `<state_dir>/<key>-daily.txt` as
    /// persistent storage. `today` must be a `YYYY-MM-DD` string.
    pub fn new(state_dir: &Path, key: &str, today: &str) -> Self {
        Self {
            path: state_dir.join(format!("{key}-daily.txt")),
            today: today.to_string(),
        }
    }

    /// Returns `true` if the action has NOT run today yet (file absent, empty,
    /// or contains a date other than today).
    pub fn should_run(&self) -> bool {
        match std::fs::read_to_string(&self.path) {
            Ok(content) => content.trim() != self.today.trim(),
            Err(_) => true,
        }
    }

    /// Mark today's run as done. Writes today's date to the state file.
    pub fn mark_done(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, format!("{}\n", self.today))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_should_run_is_true() {
        let dir = tempfile::tempdir().unwrap();
        let guard = DailyGuard::new(dir.path(), "test", "2026-06-26");
        assert!(guard.should_run());
    }

    #[test]
    fn after_mark_done_should_run_is_false() {
        let dir = tempfile::tempdir().unwrap();
        let guard = DailyGuard::new(dir.path(), "test", "2026-06-26");
        guard.mark_done().unwrap();
        assert!(!guard.should_run());
    }

    #[test]
    fn stale_date_triggers_run() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-daily.txt");
        std::fs::write(&path, "2026-06-25\n").unwrap();
        let guard = DailyGuard::new(dir.path(), "test", "2026-06-26");
        assert!(guard.should_run());
    }

    #[test]
    fn mark_done_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join("nested/state");
        let guard = DailyGuard::new(&state_dir, "test", "2026-06-26");
        assert!(guard.mark_done().is_ok());
        assert!(!guard.should_run());
    }

    #[test]
    fn different_keys_are_independent() {
        let dir = tempfile::tempdir().unwrap();
        let g1 = DailyGuard::new(dir.path(), "key1", "2026-06-26");
        let g2 = DailyGuard::new(dir.path(), "key2", "2026-06-26");
        g1.mark_done().unwrap();
        assert!(!g1.should_run());
        assert!(g2.should_run(), "key2 should still be fresh");
    }
}
