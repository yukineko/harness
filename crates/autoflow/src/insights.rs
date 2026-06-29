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
    // Sanitise the hook-supplied id so it can't traverse out of the state dir.
    let path = home().join(".session-insights").join("state").join(format!(
        "{}.json",
        harness_core::store::safe_session(session_id)
    ));
    harness_core::store::load_json(&path)
}
