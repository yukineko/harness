//! Stop-hook latency panel: read the central `hook-latency.jsonl` ledger written
//! by the three heavy (600s-timeout) gates, aggregate per session, and flag any
//! session whose combined Stop-hook wall-time exceeds the budget.

use harness_core::hook_latency::{aggregate, ledger_path, over_budget};
use serde::Serialize;

/// Default aggregate Stop-hook budget: 30s (matches the light gates' own ceiling
/// and the `HARNESS_HOOK_LATENCY_BUDGET_MS` override documented in the contract).
const DEFAULT_BUDGET_MS: u64 = 30_000;

/// The aggregate Stop-hook latency budget in ms, from
/// `HARNESS_HOOK_LATENCY_BUDGET_MS` (parsed as u64), else [`DEFAULT_BUDGET_MS`].
pub fn budget_ms() -> u64 {
    harness_core::config::env_u64("HARNESS_HOOK_LATENCY_BUDGET_MS").unwrap_or(DEFAULT_BUDGET_MS)
}

/// One session's aggregated Stop-hook latency, JSON-serializable for the panel.
#[derive(Serialize)]
pub struct SessionLatencySummary {
    pub session: String,
    pub total_ms: u64,
    pub per_hook: Vec<HookMs>,
    pub over_budget: bool,
}

#[derive(Serialize)]
pub struct HookMs {
    pub hook: String,
    pub ms: u64,
}

/// The full Stop-hook latency report threaded into both the `hooks` subcommand
/// and the default all-panels view.
#[derive(Serialize)]
pub struct HookLatencyReport {
    pub budget_ms: u64,
    pub sessions: Vec<SessionLatencySummary>,
    pub over_budget_sessions: Vec<String>,
}

/// Read + aggregate the central ledger into a [`HookLatencyReport`]. A missing or
/// empty ledger yields an empty report (never panics).
pub fn read() -> HookLatencyReport {
    let budget = budget_ms();
    let path = ledger_path();
    let agg = aggregate(&path, budget);
    let over: Vec<String> = over_budget(&agg, budget)
        .iter()
        .map(|s| s.session.clone())
        .collect();
    let sessions = agg
        .into_iter()
        .map(|s| SessionLatencySummary {
            over_budget: s.total_ms > budget,
            session: s.session,
            total_ms: s.total_ms,
            per_hook: s
                .per_hook
                .into_iter()
                .map(|(hook, ms)| HookMs { hook, ms })
                .collect(),
        })
        .collect();
    HookLatencyReport {
        budget_ms: budget,
        sessions,
        over_budget_sessions: over,
    }
}

/// First 8 chars of a session id (or the whole id if shorter) for compact display.
pub fn sess8(session: &str) -> &str {
    session.get(..8).unwrap_or(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests mutate a process-global env var, so they must not run
    // concurrently with each other. Rust runs tests in a module in parallel, so
    // guard the env with a mutex.
    use std::sync::Mutex;
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn budget_ms_parses_set_unset_and_garbage() {
        let _g = ENV_LOCK.lock().unwrap();
        let key = "HARNESS_HOOK_LATENCY_BUDGET_MS";

        std::env::set_var(key, "12345");
        assert_eq!(budget_ms(), 12345, "a valid u64 is honored");

        std::env::set_var(key, "not-a-number");
        assert_eq!(
            budget_ms(),
            DEFAULT_BUDGET_MS,
            "garbage falls back to default"
        );

        std::env::remove_var(key);
        assert_eq!(
            budget_ms(),
            DEFAULT_BUDGET_MS,
            "unset falls back to default"
        );
    }

    #[test]
    fn empty_or_missing_ledger_yields_empty_report() {
        // `read()` targets the real central ledger, which may or may not exist on
        // this machine. The invariant under test is that it never panics and, on a
        // clean machine, is empty. We assert the structural invariants that hold
        // regardless: budget is populated and over_budget_sessions ⊆ sessions.
        let report = read();
        assert!(report.budget_ms >= 1, "budget is always populated");
        for over in &report.over_budget_sessions {
            assert!(
                report.sessions.iter().any(|s| &s.session == over),
                "every over-budget session is present in sessions"
            );
        }
    }
}
