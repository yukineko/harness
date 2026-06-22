//! Per-session state, persisted across stops: the hash of the diff we last
//! asked the agent to review, plus a consecutive-round counter so the gate can
//! give up after `max_attempts` instead of trapping the agent.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SessionState {
    /// Number of consecutive review rounds forced this session.
    pub attempts: u32,
    /// Hash of the diff we last forced a review of. When the next stop carries
    /// the same hash, the agent already reviewed exactly this — let it through.
    #[serde(default)]
    pub last_hash: String,
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

pub fn load(state_dir: &Path, session: &str) -> SessionState {
    std::fs::read_to_string(path(state_dir, session))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(state_dir: &Path, session: &str, st: &SessionState) {
    let p = path(state_dir, session);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string(st) {
        let _ = std::fs::write(&p, s);
    }
}

/// Clear the counter (a clean stop with nothing left to review).
pub fn reset(state_dir: &Path, session: &str) {
    let _ = std::fs::remove_file(path(state_dir, session));
}
