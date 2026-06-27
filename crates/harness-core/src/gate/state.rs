//! Per-session state, persisted across stops so a gate can give up after
//! `max_attempts` consecutive blocks instead of trapping a stuck agent in an
//! endless fix→stop→block loop.
//!
//! One struct serves all three gates. donegate and tdd only need
//! `attempts`/`last_ts` and drive it with [`bump`]/[`reset`]; reviewgate also
//! records `last_hash` (the diff it last forced a review of) and drives it with
//! [`load`]/[`save`]/[`reset`] directly. `last_hash` defaults to the empty
//! string, so state files written by donegate/tdd round-trip unchanged.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One session's gate state.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SessionState {
    /// Number of consecutive blocks forced this session.
    pub attempts: u32,
    /// reviewgate only: hash of the diff we last forced a review of. When the
    /// next stop carries the same hash, the agent already reviewed exactly this.
    /// donegate/tdd never set it; empty string is the default.
    #[serde(default)]
    pub last_hash: String,
    /// Unix timestamp of the last block, used to reset after an idle gap.
    pub last_ts: i64,
}

fn safe(session: &str) -> String {
    session
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn path(state_dir: &Path, session: &str) -> PathBuf {
    state_dir
        .join("sessions")
        .join(format!("{}.json", safe(session)))
}

fn now() -> i64 {
    chrono::Local::now().timestamp()
}

/// Load the state for `session`, or the default (all-zero) state if none exists
/// or it can't be parsed.
pub fn load(state_dir: &Path, session: &str) -> SessionState {
    std::fs::read_to_string(path(state_dir, session))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Persist `st` for `session`. Best effort, local only.
pub fn save(state_dir: &Path, session: &str, st: &SessionState) {
    let p = path(state_dir, session);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string(st) {
        let _ = std::fs::write(&p, s);
    }
}

/// Clear the counter (a clean stop with nothing left to block on).
pub fn reset(state_dir: &Path, session: &str) {
    let _ = std::fs::remove_file(path(state_dir, session));
}

/// Record one more block for this session and return the resulting attempt
/// count. The counter is treated as 0 if more than `reset_after_secs` elapsed
/// since the last block (a fresh turn). Used by donegate/tdd; `last_hash` is
/// preserved untouched.
pub fn bump(state_dir: &Path, session: &str, reset_after_secs: i64) -> u32 {
    let mut st = load(state_dir, session);
    let t = now();
    if t - st.last_ts > reset_after_secs {
        st.attempts = 0;
    }
    st.attempts += 1;
    st.last_ts = t;
    save(state_dir, session, &st);
    st.attempts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "hc-gate-state-{}-{tag}-{}",
            std::process::id(),
            now()
        ))
    }

    #[test]
    fn bump_increments_within_window() {
        let d = dir("inc");
        assert_eq!(bump(&d, "s1", 600), 1);
        assert_eq!(bump(&d, "s1", 600), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn idle_gap_resets_counter() {
        let d = dir("idle");
        save(
            &d,
            "s1",
            &SessionState {
                attempts: 5,
                last_hash: String::new(),
                last_ts: now() - 1000,
            },
        );
        // gap (1000s) exceeds reset_after_secs (600) → reset to 0, then +1.
        assert_eq!(bump(&d, "s1", 600), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reset_removes_the_file() {
        let d = dir("reset");
        bump(&d, "s1", 600);
        reset(&d, "s1");
        assert_eq!(load(&d, "s1").attempts, 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn last_hash_round_trips() {
        let d = dir("hash");
        save(
            &d,
            "s1",
            &SessionState {
                attempts: 2,
                last_hash: "deadbeef".to_string(),
                last_ts: 123,
            },
        );
        let st = load(&d, "s1");
        assert_eq!(st.attempts, 2);
        assert_eq!(st.last_hash, "deadbeef");
        assert_eq!(st.last_ts, 123);
        let _ = std::fs::remove_dir_all(&d);
    }
}
