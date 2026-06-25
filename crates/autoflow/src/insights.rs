use std::path::PathBuf;

use harness_core::config::home;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct SessionMetrics {
    #[serde(default)]
    pub turns: u64,
    #[serde(default)]
    pub tool_events: u64,
}

pub fn load_metrics(session_id: &str) -> SessionMetrics {
    let path = PathBuf::from(home())
        .join(".session-insights")
        .join("state")
        .join(format!("{session_id}.json"));
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}
