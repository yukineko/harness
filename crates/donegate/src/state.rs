//! Per-session attempt counter, persisted across stops so the gate can give up
//! after `max_attempts` consecutive blocks instead of trapping a stuck agent in
//! an endless fix→stop→block loop.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
struct SessionState {
    attempts: u32,
    last_ts: i64,
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

fn load(p: &Path) -> SessionState {
    std::fs::read_to_string(p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Record one more block for this session and return the resulting attempt
/// count. The counter is treated as 0 if more than `reset_after_secs` elapsed
/// since the last block (a fresh turn).
pub fn bump(state_dir: &Path, session: &str, reset_after_secs: i64) -> u32 {
    let p = path(state_dir, session);
    let mut st = load(&p);
    let t = now();
    if t - st.last_ts > reset_after_secs {
        st.attempts = 0;
    }
    st.attempts += 1;
    st.last_ts = t;
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string(&st) {
        let _ = std::fs::write(&p, s);
    }
    st.attempts
}

/// Clear the counter (a clean, green stop).
pub fn reset(state_dir: &Path, session: &str) {
    let _ = std::fs::remove_file(path(state_dir, session));
}
