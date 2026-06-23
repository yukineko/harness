//! Read budgetguard's ledger for today's spend.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

fn state_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".budgetguard")
        .join("state")
}

fn ledger_path() -> PathBuf {
    state_dir().join("ledger.json")
}

#[derive(Debug, Default, Deserialize)]
struct DayEntry {
    sessions: BTreeMap<String, f64>,
}

impl DayEntry {
    fn total(&self) -> f64 {
        self.sessions.values().sum()
    }
}

#[derive(Debug, Default, Deserialize)]
struct Ledger {
    days: BTreeMap<String, DayEntry>,
}

#[derive(Debug, Serialize)]
pub struct BudgetStatus {
    pub today_usd: f64,
    pub session_count_today: usize,
    pub ledger_present: bool,
}

pub fn read(today: &str) -> BudgetStatus {
    let path = ledger_path();
    if !path.exists() {
        return BudgetStatus { today_usd: 0.0, session_count_today: 0, ledger_present: false };
    }
    let ledger: Ledger = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let day = ledger.days.get(today);
    BudgetStatus {
        today_usd: day.map(|d| d.total()).unwrap_or(0.0),
        session_count_today: day.map(|d| d.sessions.len()).unwrap_or(0),
        ledger_present: true,
    }
}
