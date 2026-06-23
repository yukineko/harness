//! Per-session state: record the HEAD SHA at session start so later hooks can
//! compute `<start>..HEAD` diffs.
//!
//! State lives in `<log_dir>/sessions/<session_id>.json`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SessionState {
    pub session_id: String,
    pub start_sha: String,
    pub project: String,
    pub started_at: String,
}

fn session_path(log_dir: &Path, session_id: &str) -> PathBuf {
    let safe: String = session_id.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    log_dir.join("sessions").join(format!("{safe}.json"))
}

pub fn load(log_dir: &Path, session_id: &str) -> Option<SessionState> {
    let p = session_path(log_dir, session_id);
    let text = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save(log_dir: &Path, state: &SessionState) -> std::io::Result<()> {
    let dir = log_dir.join("sessions");
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(state).unwrap_or_default();
    std::fs::write(session_path(log_dir, &state.session_id), json)
}
