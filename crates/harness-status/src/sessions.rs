//! Read gauge's session records for recent session info.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use harness_core::usage::ModelUsage;

fn gauge_state_dir() -> PathBuf {
    // gauge's default state_dir is `~/.gauge/store` (see gauge/src/config.rs);
    // session records live under `<state_dir>/sessions`.
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".gauge")
        .join("store")
}

#[derive(Debug, Default, Deserialize)]
struct GaugeRecord {
    session_id: String,
    project: String,
    #[serde(default)]
    models: BTreeMap<String, ModelUsage>,
    #[serde(default)]
    turns: u64,
    #[serde(default)]
    last_ts: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct SessionSummary {
    pub session_id: String,
    pub project: String,
    pub turns: u64,
    pub total_tokens: u64,
    pub last_ts: Option<String>,
    pub cost_usd: f64,
}

pub fn recent(n: usize) -> Vec<SessionSummary> {
    let dir = gauge_state_dir().join("sessions");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return vec![];
    };
    let mut records: Vec<GaugeRecord> = entries
        .flatten()
        .filter(|e| {
            e.path().extension().and_then(|x| x.to_str()) == Some("json")
        })
        .filter_map(|e| {
            std::fs::read_to_string(e.path())
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        })
        .collect();

    // Sort newest first.
    records.sort_by(|a, b| b.last_ts.cmp(&a.last_ts));
    records.truncate(n);

    records
        .into_iter()
        .map(|r| {
            let total_tokens: u64 = r.models.values().map(|u| u.total_tokens()).sum();
            let cost_usd =
                harness_core::pricing::session_cost(r.models.iter(), &[]);
            SessionSummary {
                session_id: r.session_id,
                project: r.project,
                turns: r.turns,
                total_tokens,
                last_ts: r.last_ts,
                cost_usd,
            }
        })
        .collect()
}
