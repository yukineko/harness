//! Read gauge's session records for recent session info.
//!
//! Reads gauge's canonical [`SessionRecord`] (shared via harness_core) instead
//! of a local mirror struct, and prefers budgetguard's authoritative per-session
//! cost from the shared [`Ledger`] over recomputing it with a divergent override
//! set — so the number shown here matches what the gate actually counted.

use serde::Serialize;

use harness_core::ledger::Ledger;
use harness_core::{pricing, session};

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
    let mut records = session::load_all(&session::default_state_dir());
    if records.is_empty() {
        return vec![];
    }

    // Sort newest first.
    records.sort_by(|a, b| b.last_ts.cmp(&a.last_ts));
    records.truncate(n);

    // budgetguard's ledger holds the authoritative per-session USD; reuse it so
    // this view can't diverge from the gate. Fall back to a default-rate recompute
    // only when a session isn't in the ledger (gauge ran but budgetguard didn't).
    let ledger = Ledger::load(&harness_core::ledger::default_state_dir());

    records
        .into_iter()
        .map(|r| {
            let total_tokens = r.total_tokens();
            let cost_usd = ledger
                .session_cost(&r.session_id)
                .unwrap_or_else(|| pricing::session_cost(r.models.iter(), &[]));
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
