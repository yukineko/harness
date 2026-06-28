//! Replay verification — recompute a summary's aggregates and check the pins.
//!
//! `verify` is the heart of the gate: it does NOT trust the `expect` block's
//! own bookkeeping. Instead it RECOMPUTES the phase set, error count, and total
//! cost from `steps` and checks those against the pinned `expect`. So a fixture
//! whose committed steps later drift (a new error span, a cost blowout, a
//! dropped phase) fails — which is exactly the regression signal we want.

use crate::summary::TrajectorySummary;

/// Tolerance for the cost-cap comparison — absorbs IEEE-754 summation drift so a
/// run never exceeds the cap it pinned from its own (identically summed) total.
const COST_EPSILON: f64 = 1e-9;

/// Check a summary's recomputed aggregates against its pinned `expect` block.
/// Returns `Ok(())` on a clean replay, or `Err(violations)` listing every
/// invariant that was broken (so a report can show all of them at once).
pub fn verify(summary: &TrajectorySummary) -> Result<(), Vec<String>> {
    let mut violations = Vec::new();

    // Phase set must match exactly (same phases, same first-seen order).
    let actual_phases = summary.phases();
    if actual_phases != summary.expect.phases {
        violations.push(format!(
            "phase set drifted: expected {:?}, got {:?}",
            summary.expect.phases, actual_phases
        ));
    }

    // Error count must not exceed the pinned ceiling ("no new errors").
    let errors = summary.error_count();
    if errors > summary.expect.max_error_count {
        violations.push(format!(
            "error count {} exceeds max {}",
            errors, summary.expect.max_error_count
        ));
    }

    // Total cost must not exceed the cap, when one was pinned. A small epsilon
    // absorbs floating-point summation drift (0.1 + 0.2 != 0.3 in IEEE-754), so
    // an unchanged run never trips its own pinned cap.
    if let Some(cap) = summary.expect.max_cost_usd {
        let total = summary.total_cost_usd().unwrap_or(0.0);
        if total > cap + COST_EPSILON {
            violations.push(format!("total cost {total:.6} exceeds max {cap:.6}"));
        }
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summary::{Expect, Step, TrajectorySummary};

    fn step(phase: &str, status: &str, cost: Option<f64>) -> Step {
        Step {
            span_id: format!("s-{phase}-{status}"),
            parent_id: None,
            name: "n".into(),
            phase: phase.into(),
            model: None,
            status: status.into(),
            ms: 1,
            cost_usd: cost,
        }
    }

    fn healthy() -> TrajectorySummary {
        TrajectorySummary {
            run_id: "r".into(),
            steps: vec![
                step("interpreter", "ok", Some(0.1)),
                step("worker", "verified", Some(0.2)),
                step("verifier", "ok", None),
            ],
            expect: Expect {
                phases: vec!["interpreter".into(), "worker".into(), "verifier".into()],
                max_error_count: 0,
                max_cost_usd: Some(0.3),
            },
        }
    }

    #[test]
    fn healthy_fixture_passes() {
        assert!(verify(&healthy()).is_ok());
    }

    #[test]
    fn error_step_over_ceiling_fails() {
        let mut s = healthy();
        // flip a step to an error → recomputed error_count (1) > max (0)
        s.steps[1].status = "failed".into();
        let v = verify(&s).unwrap_err();
        assert!(v.iter().any(|m| m.contains("error count")), "{v:?}");
    }

    #[test]
    fn cost_over_cap_fails() {
        let mut s = healthy();
        s.steps[0].cost_usd = Some(5.0); // total 5.2 > cap 0.3
        let v = verify(&s).unwrap_err();
        assert!(v.iter().any(|m| m.contains("cost")), "{v:?}");
    }

    #[test]
    fn phase_mismatch_fails() {
        let mut s = healthy();
        s.steps[2].phase = "tool".into(); // verifier → tool: phase set drifts
        let v = verify(&s).unwrap_err();
        assert!(v.iter().any(|m| m.contains("phase set")), "{v:?}");
    }

    #[test]
    fn no_cap_means_cost_is_not_checked() {
        let mut s = healthy();
        s.expect.max_cost_usd = None;
        s.steps[0].cost_usd = Some(99.0);
        assert!(verify(&s).is_ok());
    }
}
