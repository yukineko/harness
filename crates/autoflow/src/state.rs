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
    /// In the condukt loop (auto ≤4 times, ask user ≥5 times).
    Continuing,
    /// All condukt tasks are done; no more blocking this session.
    Done,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SessionState {
    pub phase: Phase,
    /// How many times we have prompted to run /condukt this session.
    #[serde(default)]
    pub condukt_prompts: u32,
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
