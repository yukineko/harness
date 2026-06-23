//! Persistent daily spend ledger.
//!
//! `~/.budgetguard/state/ledger.json` maps `YYYY-MM-DD` →
//! `{ sessions: { <id>: <cost_usd> } }`. On each Stop we overwrite the current
//! session's entry and recompute the day total, so the file is always consistent
//! with the latest transcript. Old days accumulate naturally (prune if desired).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DayEntry {
    /// session_id → latest cost for that session on this day.
    pub sessions: BTreeMap<String, f64>,
}

impl DayEntry {
    pub fn total(&self) -> f64 {
        self.sessions.values().sum()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Ledger {
    pub days: BTreeMap<String, DayEntry>,
}

impl Ledger {
    fn path(state_dir: &Path) -> PathBuf {
        state_dir.join("ledger.json")
    }

    pub fn load(state_dir: &Path) -> Self {
        std::fs::read_to_string(Self::path(state_dir))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, state_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(state_dir)?;
        let json = serde_json::to_string_pretty(self).unwrap_or_default();
        std::fs::write(Self::path(state_dir), json)
    }

    /// Update the session entry for `today`, return the updated day total.
    pub fn record(&mut self, session_id: &str, today: &str, cost: f64) -> f64 {
        let entry = self.days.entry(today.to_string()).or_default();
        entry.sessions.insert(session_id.to_string(), cost);
        entry.total()
    }

    pub fn day_total(&self, date: &str) -> f64 {
        self.days.get(date).map(|e| e.total()).unwrap_or(0.0)
    }
}
