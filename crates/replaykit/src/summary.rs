//! The portable trajectory summary — the committed fixture format.
//!
//! A [`TrajectorySummary`] is what we extract from a run's spans and commit to
//! the repo as `evals/replay/fixtures/<id>.json`. It is deliberately
//! self-contained (no tracekit dependency at replay time): the ordered `steps`
//! carry the raw observations, and the `expect` block pins the invariants a
//! replay must hold. Crucially the aggregates in `expect` are *re-derivable*
//! from `steps`, so `verify` can recompute and compare — making the fixture a
//! self-test of the aggregation logic, not just a static snapshot read.

use serde::{Deserialize, Serialize};

use crate::trace::Span;

/// One step of a trajectory: a span flattened to the fields a replay reasons
/// about. Ordered within a summary by `end_unix_ms` ascending.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub span_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub name: String,
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub status: String,
    #[serde(default)]
    pub ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

impl Step {
    /// Mirror of [`Span::is_error`]: status is neither "ok" nor "verified".
    pub fn is_error(&self) -> bool {
        let s = self.status.to_lowercase();
        s != "ok" && s != "verified"
    }
}

/// The pinned invariants of a replay. `phases` is the run's distinct phase set
/// (first-seen order); `max_error_count` pins "no new errors" by recording the
/// observed count; `max_cost_usd` caps total cost when one was observed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expect {
    pub phases: Vec<String>,
    pub max_error_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<f64>,
}

/// A portable, self-verifying summary of one recorded run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectorySummary {
    pub run_id: String,
    pub steps: Vec<Step>,
    pub expect: Expect,
}

impl TrajectorySummary {
    /// Build a summary from a run's spans: steps sorted by `end_unix_ms`, with
    /// the `expect` block derived from the observed aggregates (so the snapshot
    /// pins exactly the current behaviour).
    pub fn from_spans(run_id: impl Into<String>, spans: &[Span]) -> Self {
        let mut steps: Vec<Step> = spans
            .iter()
            .map(|s| Step {
                span_id: s.span_id.clone(),
                parent_id: s.parent_id.clone(),
                name: s.name.clone(),
                phase: s.phase.clone(),
                model: s.model.clone(),
                status: s.status.clone(),
                ms: s.ms,
                cost_usd: s.cost_usd,
            })
            .collect();
        // Stable sort by end_unix_ms keeps same-timestamp spans in record order.
        let order: Vec<u64> = spans.iter().map(|s| s.end_unix_ms).collect();
        let mut idx: Vec<usize> = (0..steps.len()).collect();
        idx.sort_by_key(|&i| order[i]);
        steps = idx.into_iter().map(|i| steps[i].clone()).collect();

        let summary = TrajectorySummary {
            run_id: run_id.into(),
            steps,
            // Filled below from the derived accessors.
            expect: Expect {
                phases: Vec::new(),
                max_error_count: 0,
                max_cost_usd: None,
            },
        };
        let expect = Expect {
            phases: summary.phases(),
            max_error_count: summary.error_count(),
            max_cost_usd: summary.total_cost_usd(),
        };
        TrajectorySummary { expect, ..summary }
    }

    /// Distinct phases in first-seen order, recomputed from `steps`.
    pub fn phases(&self) -> Vec<String> {
        let mut seen = Vec::new();
        for step in &self.steps {
            if !seen.iter().any(|p| p == &step.phase) {
                seen.push(step.phase.clone());
            }
        }
        seen
    }

    /// Number of error steps, recomputed from `steps`.
    pub fn error_count(&self) -> usize {
        self.steps.iter().filter(|s| s.is_error()).count()
    }

    /// Total cost across steps, recomputed from `steps`. `None` when no step
    /// carried a cost (so we never pin a cap on a run that was never priced).
    pub fn total_cost_usd(&self) -> Option<f64> {
        let mut any = false;
        let mut total = 0.0;
        for s in &self.steps {
            if let Some(c) = s.cost_usd {
                any = true;
                total += c;
            }
        }
        if any {
            Some(total)
        } else {
            None
        }
    }

    /// Total wall-time across steps, recomputed from `steps`.
    pub fn total_ms(&self) -> u64 {
        self.steps.iter().map(|s| s.ms).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::Span;

    fn span(
        span_id: &str,
        phase: &str,
        status: &str,
        ms: u64,
        end: u64,
        cost: Option<f64>,
    ) -> Span {
        Span {
            run_id: "r".into(),
            span_id: span_id.into(),
            parent_id: None,
            name: format!("n-{span_id}"),
            phase: phase.into(),
            model: None,
            task_id: None,
            ms,
            cost_usd: cost,
            status: status.into(),
            end_unix_ms: end,
        }
    }

    #[test]
    fn from_spans_orders_by_end_unix_ms() {
        // recorded out of order (end 3,1,2) → steps must come out 1,2,3
        let spans = vec![
            span("c", "verifier", "ok", 1, 3, None),
            span("a", "interpreter", "ok", 1, 1, None),
            span("b", "worker", "ok", 1, 2, None),
        ];
        let s = TrajectorySummary::from_spans("run", &spans);
        let ids: Vec<&str> = s.steps.iter().map(|x| x.span_id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn phases_are_first_seen_distinct() {
        let spans = vec![
            span("a", "interpreter", "ok", 1, 1, None),
            span("b", "worker", "ok", 1, 2, None),
            span("c", "worker", "ok", 1, 3, None),
            span("d", "verifier", "ok", 1, 4, None),
        ];
        let s = TrajectorySummary::from_spans("run", &spans);
        assert_eq!(s.phases(), vec!["interpreter", "worker", "verifier"]);
        assert_eq!(s.expect.phases, vec!["interpreter", "worker", "verifier"]);
    }

    #[test]
    fn error_count_and_cost_are_derived() {
        let spans = vec![
            span("a", "interpreter", "ok", 2, 1, Some(0.10)),
            span("b", "worker", "failed", 3, 2, Some(0.20)),
            span("c", "verifier", "verified", 1, 3, None),
        ];
        let s = TrajectorySummary::from_spans("run", &spans);
        assert_eq!(s.error_count(), 1);
        assert_eq!(s.expect.max_error_count, 1);
        assert!((s.total_cost_usd().unwrap() - 0.30).abs() < 1e-9);
        assert!((s.expect.max_cost_usd.unwrap() - 0.30).abs() < 1e-9);
        assert_eq!(s.total_ms(), 6);
    }

    #[test]
    fn cost_is_none_when_no_step_priced() {
        let spans = vec![span("a", "worker", "ok", 1, 1, None)];
        let s = TrajectorySummary::from_spans("run", &spans);
        assert_eq!(s.total_cost_usd(), None);
        assert_eq!(s.expect.max_cost_usd, None);
    }
}
