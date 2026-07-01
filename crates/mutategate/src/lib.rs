//! mutategate — a mutation-testing **kill-rate gate** over `cargo-mutants` output.
//!
//! Golden/regression suites prove the code *still does what it did*; they say
//! nothing about whether the tests would *catch a fault* if one were introduced.
//! Mutation testing closes that gap: it injects small faults ("mutants") and
//! checks whether the existing tests fail (the mutant is "caught") or still pass
//! (the mutant "survives" / is "missed"). The fraction of viable mutants caught
//! is the **kill-rate** (a.k.a. mutation score); a low score means the tests are
//! weak regardless of how green they look. (Cf. Meta's ACH and PRIMG,
//! arXiv:2505.05584.)
//!
//! This crate deliberately does **not** implement a mutation engine — it stands on
//! the standard Rust tool [`cargo-mutants`](https://mutants.rs). Its only job is
//! the *gate*: parse `cargo-mutants`'s `outcomes.json`, compute the kill-rate, and
//! decide pass/fail against a threshold. That parse→score→exit-code logic is pure
//! and unit-tested here on fixed sample JSON, so the gate is deterministic and
//! runs without invoking the (slow) mutation engine.
//!
//! ## outcomes.json shape (the bits we rely on)
//! `cargo-mutants` writes `mutants.out/outcomes.json`:
//! ```json
//! {
//!   "outcomes": [
//!     { "scenario": "Baseline",              "summary": "Success" },
//!     { "scenario": { "Mutant": { .. } },    "summary": "CaughtMutant" },
//!     { "scenario": { "Mutant": { .. } },    "summary": "MissedMutant" }
//!   ]
//! }
//! ```
//! `summary` is one of `Success`, `CaughtMutant`, `MissedMutant`, `Timeout`,
//! `Unviable`, `Failure`. The `Baseline` scenario (an unmutated build) is not a
//! mutant and is excluded from the score. We count directly from the `outcomes`
//! array rather than trusting the top-level summary counts, so the score is
//! reproducible from the raw records alone.

use serde::Deserialize;

/// Small epsilon so a kill-rate that is exactly the threshold passes despite
/// binary-float rounding (e.g. `0.7999999` when the true value is `0.8`).
const KILL_RATE_EPSILON: f64 = 1e-9;

/// Raw top-level `outcomes.json` document. Only `outcomes` is needed.
#[derive(Debug, Deserialize)]
struct RawLabOutcome {
    #[serde(default)]
    outcomes: Vec<RawScenarioOutcome>,
}

/// Raw per-scenario record. `scenario` is either the string `"Baseline"` or an
/// object `{ "Mutant": { .. } }`; we keep it as untyped JSON and only ask whether
/// it is a mutant.
#[derive(Debug, Deserialize)]
struct RawScenarioOutcome {
    #[serde(default)]
    scenario: serde_json::Value,
    #[serde(default)]
    summary: Option<String>,
}

/// Tallied mutant outcomes (baseline excluded).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MutationSummary {
    /// Mutant detected by a test failure — a "kill".
    pub caught: u64,
    /// Mutant that survived: all tests still passed. This is what the gate
    /// punishes.
    pub missed: u64,
    /// Mutant that made the test run hang past the timeout. Counted as killed:
    /// the mutation produced observable, test-exposed misbehaviour.
    pub timeout: u64,
    /// Mutant that did not compile. Excluded from the denominator — an unviable
    /// mutant carries no signal about test strength.
    pub unviable: u64,
    /// A non-baseline scenario that reported plain `Success` (should not normally
    /// occur for mutants); tracked for completeness, excluded from scoring.
    pub success: u64,
    /// A scenario whose harness itself failed. Tracked, excluded from scoring.
    pub failure: u64,
}

impl MutationSummary {
    /// Mutants that carry signal about test strength: caught + missed + timeout.
    /// Unviable mutants (they don't compile) and bookkeeping states are excluded.
    pub fn viable(&self) -> u64 {
        self.caught + self.missed + self.timeout
    }

    /// Mutants the tests killed: caught + timeout.
    pub fn killed(&self) -> u64 {
        self.caught + self.timeout
    }

    /// Kill-rate = killed / viable, or `None` when there are no viable mutants
    /// (an undefined score — treated by the gate as a failure, since a run that
    /// produced no scorable mutants gates nothing).
    pub fn kill_rate(&self) -> Option<f64> {
        let viable = self.viable();
        if viable == 0 {
            None
        } else {
            Some(self.killed() as f64 / viable as f64)
        }
    }
}

/// Count mutant outcomes from raw `outcomes.json` text.
///
/// Baseline scenarios are skipped. Unknown/absent `summary` values are ignored
/// (forward-compatible with new `cargo-mutants` states).
pub fn parse_outcomes(json: &str) -> anyhow::Result<MutationSummary> {
    let lab: RawLabOutcome = serde_json::from_str(json)?;
    let mut s = MutationSummary::default();
    for o in &lab.outcomes {
        if !is_mutant(&o.scenario) {
            continue;
        }
        match o.summary.as_deref() {
            Some("CaughtMutant") => s.caught += 1,
            Some("MissedMutant") => s.missed += 1,
            Some("Timeout") => s.timeout += 1,
            Some("Unviable") => s.unviable += 1,
            Some("Success") => s.success += 1,
            Some("Failure") => s.failure += 1,
            _ => {}
        }
    }
    Ok(s)
}

/// A scenario is a mutant iff its JSON is an object carrying a `"Mutant"` key
/// (the `Baseline` scenario serialises as the bare string `"Baseline"`).
fn is_mutant(scenario: &serde_json::Value) -> bool {
    scenario.get("Mutant").is_some()
}

/// The gate's verdict for a run.
#[derive(Debug, Clone)]
pub struct GateOutcome {
    pub summary: MutationSummary,
    /// Measured kill-rate, or `None` when no viable mutants were produced.
    pub kill_rate: Option<f64>,
    /// The minimum kill-rate required to pass.
    pub threshold: f64,
    /// Whether the gate passes.
    pub passed: bool,
    /// Human-readable one-line explanation.
    pub reason: String,
}

/// Decide pass/fail for a tally against a minimum kill-rate.
///
/// Fails when there are no viable mutants (nothing was measured) or when the
/// kill-rate is below `threshold`. The comparison is `>=` with a tiny epsilon so
/// hitting the threshold exactly passes.
pub fn evaluate(summary: MutationSummary, threshold: f64) -> GateOutcome {
    match summary.kill_rate() {
        None => GateOutcome {
            reason: format!(
                "no viable mutants produced ({} unviable, {} caught, {} missed, {} timeout) — nothing to score",
                summary.unviable, summary.caught, summary.missed, summary.timeout
            ),
            kill_rate: None,
            threshold,
            passed: false,
            summary,
        },
        Some(kr) => {
            let passed = kr + KILL_RATE_EPSILON >= threshold;
            let reason = if passed {
                format!(
                    "kill-rate {:.1}% >= {:.1}% ({} killed / {} viable; {} missed survived)",
                    kr * 100.0,
                    threshold * 100.0,
                    summary.killed(),
                    summary.viable(),
                    summary.missed,
                )
            } else {
                format!(
                    "kill-rate {:.1}% < {:.1}% ({} missed mutant(s) survived out of {} viable) — tests too weak",
                    kr * 100.0,
                    threshold * 100.0,
                    summary.missed,
                    summary.viable(),
                )
            };
            GateOutcome {
                kill_rate: Some(kr),
                threshold,
                passed,
                reason,
                summary,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A realistic (trimmed) `outcomes.json`: one baseline + a mix of mutant
    /// states. Baseline must be ignored; the score is computed from mutants only.
    const SAMPLE: &str = r#"{
      "outcomes": [
        { "scenario": "Baseline", "summary": "Success" },
        { "scenario": { "Mutant": { "file": "src/a.rs", "line": 1 } }, "summary": "CaughtMutant" },
        { "scenario": { "Mutant": { "file": "src/a.rs", "line": 2 } }, "summary": "CaughtMutant" },
        { "scenario": { "Mutant": { "file": "src/a.rs", "line": 3 } }, "summary": "Timeout" },
        { "scenario": { "Mutant": { "file": "src/b.rs", "line": 9 } }, "summary": "MissedMutant" },
        { "scenario": { "Mutant": { "file": "src/b.rs", "line": 4 } }, "summary": "Unviable" }
      ],
      "total_mutants": 5, "caught": 2, "missed": 1, "timeout": 1, "unviable": 1
    }"#;

    #[test]
    fn parse_counts_mutants_and_ignores_baseline() {
        let s = parse_outcomes(SAMPLE).unwrap();
        assert_eq!(s.caught, 2);
        assert_eq!(s.missed, 1);
        assert_eq!(s.timeout, 1);
        assert_eq!(s.unviable, 1);
        assert_eq!(
            s.success, 0,
            "baseline Success must not be counted as a mutant"
        );
    }

    #[test]
    fn viable_and_killed_exclude_unviable() {
        let s = parse_outcomes(SAMPLE).unwrap();
        // viable = caught(2) + missed(1) + timeout(1) = 4  (unviable excluded)
        assert_eq!(s.viable(), 4);
        // killed = caught(2) + timeout(1) = 3
        assert_eq!(s.killed(), 3);
    }

    #[test]
    fn kill_rate_is_killed_over_viable() {
        let s = parse_outcomes(SAMPLE).unwrap();
        assert_eq!(s.kill_rate(), Some(3.0 / 4.0)); // 0.75
    }

    #[test]
    fn gate_fails_when_below_threshold() {
        let s = parse_outcomes(SAMPLE).unwrap();
        let g = evaluate(s, 0.80); // 0.75 < 0.80
        assert!(!g.passed);
        assert!(g.reason.contains("missed"));
    }

    #[test]
    fn gate_passes_when_at_or_above_threshold() {
        let s = parse_outcomes(SAMPLE).unwrap();
        let g = evaluate(s, 0.75); // exactly at threshold -> pass
        assert!(
            g.passed,
            "hitting the threshold exactly must pass: {}",
            g.reason
        );
    }

    #[test]
    fn all_caught_is_full_kill_rate() {
        let json = r#"{ "outcomes": [
            { "scenario": "Baseline", "summary": "Success" },
            { "scenario": { "Mutant": {} }, "summary": "CaughtMutant" },
            { "scenario": { "Mutant": {} }, "summary": "CaughtMutant" }
        ] }"#;
        let s = parse_outcomes(json).unwrap();
        assert_eq!(s.kill_rate(), Some(1.0));
        assert!(evaluate(s, 1.0).passed);
    }

    #[test]
    fn no_viable_mutants_fails_the_gate() {
        // Only an unviable mutant: kill-rate is undefined, gate must fail loudly.
        let json = r#"{ "outcomes": [
            { "scenario": "Baseline", "summary": "Success" },
            { "scenario": { "Mutant": {} }, "summary": "Unviable" }
        ] }"#;
        let s = parse_outcomes(json).unwrap();
        assert_eq!(s.kill_rate(), None);
        let g = evaluate(s, 0.80);
        assert!(!g.passed);
        assert!(g.reason.contains("no viable mutants"));
    }

    #[test]
    fn empty_outcomes_fails_the_gate() {
        let s = parse_outcomes(r#"{ "outcomes": [] }"#).unwrap();
        assert_eq!(s, MutationSummary::default());
        assert!(!evaluate(s, 0.80).passed);
    }

    #[test]
    fn unknown_summary_states_are_ignored() {
        let json = r#"{ "outcomes": [
            { "scenario": { "Mutant": {} }, "summary": "CaughtMutant" },
            { "scenario": { "Mutant": {} }, "summary": "SomeFutureState" },
            { "scenario": { "Mutant": {} } }
        ] }"#;
        let s = parse_outcomes(json).unwrap();
        assert_eq!(s.caught, 1);
        assert_eq!(s.viable(), 1);
    }

    #[test]
    fn malformed_json_is_an_error() {
        assert!(parse_outcomes("not json").is_err());
    }
}
