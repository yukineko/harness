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
    let path = home()
        .join(".session-insights")
        .join("state")
        .join(format!("{session_id}.json"));
    harness_core::store::load_json(&path)
}
