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
    /// How many times we have prompted to run /backlog this session.
    #[serde(default)]
    pub backlog_prompts: u32,
}

fn state_path(state_dir: &Path, session_id: &str) -> PathBuf {
    state_dir.join(format!("{session_id}.json"))
}

pub fn load(state_dir: &Path, session_id: &str) -> SessionState {
    harness_core::store::load_json(&state_path(state_dir, session_id))
}

pub fn save(state_dir: &Path, session_id: &str, s: &SessionState) {
    harness_core::store::save_json(&state_path(state_dir, session_id), s);
}
