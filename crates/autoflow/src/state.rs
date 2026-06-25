use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// No action taken yet this session.
    #[default]
    Idle,
    /// Blocked once with a /record prompt; next Stop will check condukt.
    RecordRequested,
    /// Condukt check done (blocked or allowed); no more blocking this session.
    Done,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SessionState {
    pub phase: Phase,
}

fn state_path(state_dir: &Path, session_id: &str) -> PathBuf {
    state_dir.join(format!("{session_id}.json"))
}

pub fn load(state_dir: &Path, session_id: &str) -> SessionState {
    std::fs::read_to_string(state_path(state_dir, session_id))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(state_dir: &Path, session_id: &str, s: &SessionState) {
    if let Some(parent) = state_path(state_dir, session_id).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(s) {
        let _ = std::fs::write(state_path(state_dir, session_id), text);
    }
}
