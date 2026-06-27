//! Read budgetguard's ledger for today's spend.
//!
//! Uses budgetguard's canonical [`Ledger`] type (shared via harness_core) so
//! this view can't drift from the writer's schema.

use serde::Serialize;

use harness_core::ledger::{self, Ledger};

#[derive(Debug, Serialize)]
pub struct BudgetStatus {
    pub today_usd: f64,
    pub session_count_today: usize,
    pub ledger_present: bool,
}

pub fn read(today: &str) -> BudgetStatus {
    let state_dir = ledger::default_state_dir();
    if !state_dir.join("ledger.json").exists() {
        return BudgetStatus { today_usd: 0.0, session_count_today: 0, ledger_present: false };
    }
    let ledger = Ledger::load(&state_dir);
    let day = ledger.days.get(today);
    BudgetStatus {
        today_usd: day.map(|d| d.total()).unwrap_or(0.0),
        session_count_today: day.map(|d| d.sessions.len()).unwrap_or(0),
        ledger_present: true,
    }
}
