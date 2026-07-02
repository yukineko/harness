//! Replan vs escalate-model classifier (phase-4 DoD#1).
//!
//! When a verifier fails a task a second/third time, the `/condukt` skill's
//! reflux loop needs a deterministic answer to "what next?" before it burns
//! another LLM turn deciding: keep retrying with a stronger model, or give up
//! on the current task shape and hand it back to the interpreter for a
//! replan? Two failure shapes make "escalate the model" the wrong move:
//!
//! 1. The worker is **already at the top tier** (`opus`) and still failing —
//!    there is no stronger tier left to escalate to, so retrying with the
//!    same ceiling model would just repeat the failure.
//! 2. The failure **reason itself says the task is mis-scoped** — the
//!    `done_criteria` doesn't match what the task's `touched_files` can
//!    actually deliver. A stronger model cannot implement its way out of a
//!    scope mismatch; the task needs to be re-decomposed.
//!
//! Anything else — an assertion failure, a compile error, a type mismatch —
//! looks like an ordinary implementation bug that a stronger model has a
//! real shot at fixing, so it escalates the model instead.
//!
//! This module is the deterministic core only: given the reflux facts
//! (`reason`, `failed_tests`, `model_tier`) it computes the resolution. No LLM
//! call, no I/O, no clock — pure function of its arguments. How the resolution
//! is *acted on* (spawning a replan vs. re-dispatching a stronger-model worker)
//! is the `/condukt` SKILL orchestration's job, not this module's.
//!
//! phase-4 DoD#2 wires this classifier into the reflux cascade: when
//! [`classify_failure`] resolves to [`Resolution::Replan`],
//! [`build_replan_handoff`] formats the reflux facts into a [`ReplanHandoff`]
//! that explicitly instructs the interpreter to produce a *brand-new*
//! decomposition — a different approach, a different scope — rather than
//! re-running the original decomposition verbatim (which would just
//! reproduce the same failure). The `condukt replan handoff` CLI subcommand
//! (see `main.rs`) exposes this to the `/condukt` skill's cascade, mirroring
//! how `condukt verify digest` exposes `verify::distill_failure`: the
//! FORMATTING here is deterministic Rust, the actual re-decomposition stays
//! the interpreter LLM's job.

use serde::Serialize;

/// Known model tiers, cheapest → strongest. Kept local (rather than reusing
/// `verify::TIERS`) since that table is private to `verify.rs` and this
/// module's tier concern (top-tier detection) is narrower than the verifier's
/// (distinct-model resolution).
const TIERS: [&str; 3] = ["haiku", "sonnet", "opus"];

/// The top tier: once a worker is here and still failing, there is no
/// stronger tier left to escalate to.
pub const TOP_TIER: &str = "opus";

/// Markers that indicate a failure `reason` is about a scope / done_criteria
/// mismatch — the task's touched_files/scope cannot actually satisfy what
/// done_criteria demands — rather than a fixable implementation bug.
/// Bilingual (English + Japanese) to match the rest of condukt's classifiers
/// (see `verify::BEHAVIORAL_MARKERS`). Deliberately COMPOUND phrases (not the
/// bare words "mismatch" / "不一致" alone): a plain compiler diagnostic like
/// "mismatched types" / "型不一致" is an ordinary implementation bug (case
/// (c), EscalateModel), not a scope mismatch, and a bare-word marker would
/// misclassify it as a scope mismatch (case (b), Replan).
const SCOPE_MISMATCH_MARKERS: &[&str] = &[
    // English
    "scope mismatch",
    "out of scope",
    "out-of-scope",
    "outside the scope",
    "outside scope",
    "not in scope",
    "done_criteria mismatch",
    "cannot satisfy done_criteria",
    "can't satisfy done_criteria",
    "does not satisfy done_criteria",
    "doesn't satisfy done_criteria",
    "unsatisfiable",
    "unmet done_criteria",
    "done_criteria cannot be met",
    "done_criteria can not be met",
    // Japanese
    "スコープ不一致",
    "スコープの不一致",
    "スコープ外",
    "対象外",
    "範囲外",
    "満たせない",
];

/// The two possible resolutions for a failing task's reflux.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    /// Give up on the current task shape and hand it back for re-decomposition.
    Replan,
    /// Retry with a stronger model tier.
    EscalateModel,
}

/// The deterministic classification of a failing task's reflux facts.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FailureClassification {
    /// The chosen resolution.
    pub resolution: Resolution,
    /// Human-readable explanation of the decision (mirrors
    /// `consensus::Consensus::reason` / `verify::FailureDigest`'s design: the
    /// FORMATTING is deterministic Rust, the fix DECISION stays with the LLM).
    pub reason: String,
}

/// Collapse a model string to its canonical tier keyword when recognised
/// (e.g. `"claude-opus-4"` → `"opus"`), else the trimmed lowercase string.
/// Empty/garbage input collapses to an empty/garbage string, never panics.
fn canonical_tier(model_tier: &str) -> String {
    let m = model_tier.trim().to_lowercase();
    for t in TIERS {
        if m.contains(t) {
            return t.to_string();
        }
    }
    m
}

/// True iff `model_tier` denotes the top tier (`opus`) — no higher tier to
/// escalate to.
fn is_top_tier(model_tier: &str) -> bool {
    canonical_tier(model_tier) == TOP_TIER
}

/// True iff `reason` carries any scope/done_criteria-mismatch marker.
/// Case-insensitive; empty input never matches (never panics).
fn reason_indicates_scope_mismatch(reason: &str) -> bool {
    let lower = reason.to_lowercase();
    SCOPE_MISMATCH_MARKERS
        .iter()
        .any(|m| lower.contains(&m.to_lowercase()))
}

/// Classify a failing task's reflux facts into a [`Resolution`].
///
/// Pure and deterministic: no LLM, no network, no filesystem, no clock.
/// Handles empty / garbage input gracefully (falls through to
/// `EscalateModel`) and never panics.
///
/// Decision order:
/// 1. `model_tier` is already the top tier (`opus`) and the task is still
///    failing → [`Resolution::Replan`] (no stronger tier left).
/// 2. `reason` carries a scope/done_criteria-mismatch marker →
///    [`Resolution::Replan`] (a stronger model can't fix a mis-scoped task).
/// 3. Otherwise → [`Resolution::EscalateModel`] (looks like an ordinary
///    implementation bug — assertion failure, compile error, type mismatch —
///    that a stronger model has a real shot at fixing).
///
/// `failed_tests` is accepted for symmetry with the reflux shape (and future
/// callers) but does not currently change the decision; it is not inspected
/// beyond being a plain string argument (kept in the signature so this
/// function stays the single deterministic entry point for the reflux facts).
pub fn classify_failure(
    reason: &str,
    failed_tests: &str,
    model_tier: &str,
) -> FailureClassification {
    let _ = failed_tests; // reserved for future refinement; not yet load-bearing.

    if is_top_tier(model_tier) {
        return FailureClassification {
            resolution: Resolution::Replan,
            reason: format!(
                "model_tier '{model_tier}' is already the top tier ('{TOP_TIER}') and still \
                 failing — no stronger tier to escalate to → replan"
            ),
        };
    }

    if reason_indicates_scope_mismatch(reason) {
        return FailureClassification {
            resolution: Resolution::Replan,
            reason: "reason indicates a done_criteria/scope mismatch, not a fixable \
                      implementation bug — a stronger model cannot implement its way out of a \
                      mis-scoped task → replan"
                .to_string(),
        };
    }

    FailureClassification {
        resolution: Resolution::EscalateModel,
        reason: format!(
            "looks like an ordinary implementation bug (model_tier '{model_tier}' is below the \
             top tier) → escalate to a stronger model for a retry"
        ),
    }
}

/// A deterministic handoff for the `/condukt` skill's reflux cascade,
/// produced only when [`classify_failure`] resolves to [`Resolution::Replan`].
///
/// Carries the reflux facts forward (so the interpreter has the failure
/// context to reason about) plus an explicit [`Self::instruction`]: the
/// interpreter must produce a **new** decomposition with a **different
/// approach and different scope**, not re-run the original decomposition
/// verbatim. Re-running the same task shape against the same failure would
/// just reproduce it — that is precisely the case
/// [`classify_failure`] has already ruled out an `EscalateModel` retry for.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReplanHandoff {
    /// The classification that triggered this handoff. Always carries
    /// `resolution: Resolution::Replan` (see [`build_replan_handoff`]).
    pub classification: FailureClassification,
    /// The verifier/worker failure reason, carried forward verbatim.
    pub reason: String,
    /// The failing-test output, carried forward verbatim.
    pub failed_tests: String,
    /// The `git diff` of the failed attempt, carried forward verbatim.
    pub diff: String,
    /// The original task's `done_criteria`, carried forward verbatim (empty
    /// string if not supplied — this function never panics on missing input).
    pub done_criteria: String,
    /// A short human summary of the original task, carried forward verbatim.
    pub task_summary: String,
    /// The explicit re-decomposition instruction for the interpreter
    /// (Phase 1): produce a NEW decomposition — a different approach and a
    /// different scope than the original — rather than re-running the
    /// original decomposition as-is.
    pub instruction: String,
}

/// Build a [`ReplanHandoff`] from a failing task's reflux facts, IF AND ONLY
/// IF [`classify_failure`] resolves those facts to [`Resolution::Replan`].
///
/// Pure and deterministic: no LLM call, no I/O, no clock — a formatting
/// function of its arguments only (mirrors `verify::distill_failure`'s
/// design: the FORMATTING is deterministic Rust, the fix/re-decomposition
/// DECISION stays with the LLM). Never panics on empty/garbage input.
///
/// - `Ok(handoff)` — the failure resolves to `Replan`; `handoff.instruction`
///   explicitly tells the interpreter to build a NEW decomposition (different
///   approach, different scope), not to re-run the original one.
/// - `Err(classification)` — the failure resolves to `EscalateModel` instead;
///   no handoff is built (there is nothing to replan yet), and the caller
///   gets the classification back so it can still report the resolution.
pub fn build_replan_handoff(
    reason: &str,
    failed_tests: &str,
    diff: &str,
    model_tier: &str,
    done_criteria: &str,
    task_summary: &str,
) -> Result<ReplanHandoff, FailureClassification> {
    let classification = classify_failure(reason, failed_tests, model_tier);
    if classification.resolution != Resolution::Replan {
        return Err(classification);
    }

    let summary_display = if task_summary.trim().is_empty() {
        "(no task summary given)"
    } else {
        task_summary
    };

    let instruction = format!(
        "REPLAN, do not retry: task \"{summary_display}\" failed under model_tier \
         '{model_tier}' — {class_reason} Do NOT simply re-run the original decomposition \
         (same tasks, same touched_files, same approach); that would reproduce the same \
         failure. Instead, hand this failure_context (reason, failed_tests, diff, \
         done_criteria below) back to the interpreter (Phase 1) and have it produce a BRAND \
         NEW decomposition that takes a DIFFERENT APPROACH and a DIFFERENT SCOPE (different \
         touched_files / task boundaries) than the original decomposition.",
        class_reason = classification.reason,
    );

    Ok(ReplanHandoff {
        classification,
        reason: reason.to_string(),
        failed_tests: failed_tests.to_string(),
        diff: diff.to_string(),
        done_criteria: done_criteria.to_string(),
        task_summary: task_summary.to_string(),
        instruction,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── (a) top tier + still failing → Replan ──────────────────────────────

    #[test]
    fn top_tier_still_failing_replans() {
        let c = classify_failure("assertion `left == right` failed", "foo::bar", "opus");
        assert_eq!(c.resolution, Resolution::Replan);
        assert!(c.reason.contains("opus"), "{}", c.reason);
        assert!(c.reason.to_lowercase().contains("top tier"), "{}", c.reason);
    }

    #[test]
    fn top_tier_recognised_from_full_model_string() {
        // Canonical-tier matching must recognise a full model id, not just the
        // bare tier keyword (mirrors verify::canonical's `.contains(t)` policy).
        let c = classify_failure("boom", "", "claude-opus-4-20250101");
        assert_eq!(c.resolution, Resolution::Replan);
    }

    #[test]
    fn top_tier_case_insensitive() {
        let c = classify_failure("boom", "", "OPUS");
        assert_eq!(c.resolution, Resolution::Replan);
    }

    // ── (b) scope / done_criteria mismatch → Replan ────────────────────────

    #[test]
    fn scope_mismatch_reason_replans_even_below_top_tier() {
        let c = classify_failure(
            "done_criteria demands a public API in a file outside touched_files — scope mismatch",
            "",
            "sonnet",
        );
        assert_eq!(c.resolution, Resolution::Replan);
        assert!(
            c.reason.to_lowercase().contains("mismatch")
                || c.reason.to_lowercase().contains("scope"),
            "{}",
            c.reason
        );
    }

    #[test]
    fn scope_mismatch_reason_detected_case_insensitive() {
        let c = classify_failure("SCOPE MISMATCH: touched_files too narrow", "", "haiku");
        assert_eq!(c.resolution, Resolution::Replan);
    }

    #[test]
    fn done_criteria_unsatisfiable_reason_replans() {
        let c = classify_failure(
            "the task cannot satisfy done_criteria with the files in scope",
            "",
            "sonnet",
        );
        assert_eq!(c.resolution, Resolution::Replan);
    }

    #[test]
    fn japanese_scope_mismatch_marker_replans() {
        let c = classify_failure(
            "done_criteria とタスクのスコープ不一致のため実装不可能",
            "",
            "sonnet",
        );
        assert_eq!(c.resolution, Resolution::Replan);
    }

    // ── (c) ordinary implementation bug, below top tier → EscalateModel ────

    #[test]
    fn assertion_failure_below_top_tier_escalates_model() {
        let c = classify_failure(
            "assertion `left == right` failed\n  left: 3\n right: 4",
            "foo::bar",
            "sonnet",
        );
        assert_eq!(c.resolution, Resolution::EscalateModel);
        assert!(c.reason.contains("sonnet"), "{}", c.reason);
    }

    #[test]
    fn compile_error_below_top_tier_escalates_model() {
        let c = classify_failure("error[E0308]: mismatched types", "", "haiku");
        assert_eq!(c.resolution, Resolution::EscalateModel);
    }

    #[test]
    fn type_mismatch_below_top_tier_escalates_model() {
        let c = classify_failure("expected `String`, found `&str`", "", "sonnet");
        assert_eq!(c.resolution, Resolution::EscalateModel);
    }

    // ── defensive: empty / garbage input never panics ──────────────────────

    #[test]
    fn empty_inputs_do_not_panic_and_escalate_model() {
        let c = classify_failure("", "", "");
        // An empty model_tier is not recognised as top tier, and an empty
        // reason carries no scope-mismatch marker → falls through to escalate.
        assert_eq!(c.resolution, Resolution::EscalateModel);
    }

    #[test]
    fn garbage_inputs_do_not_panic() {
        let garbage_reason = "\u{0}\t!@#$%^&*()_+{}|:<>?~`-=[]\\;',./\u{1b}[31m";
        let garbage_tier = "🦀🦀🦀not-a-real-tier🦀🦀🦀";
        let c = classify_failure(garbage_reason, garbage_reason, garbage_tier);
        assert_eq!(c.resolution, Resolution::EscalateModel);
        assert!(!c.reason.is_empty());
    }

    #[test]
    fn whitespace_only_model_tier_is_not_top_tier() {
        let c = classify_failure("boom", "", "   ");
        assert_eq!(c.resolution, Resolution::EscalateModel);
    }

    // ── decision order: (a) wins over (b) when both apply ───────────────────

    #[test]
    fn top_tier_wins_over_scope_mismatch_reason() {
        // Even a scope-mismatch-sounding reason still resolves to Replan when
        // already at top tier (both rules agree on Replan here, but this pins
        // that rule (a) is checked first / does not require rule (b) to fire).
        let c = classify_failure("scope mismatch: done_criteria unmet", "", "opus");
        assert_eq!(c.resolution, Resolution::Replan);
        assert!(c.reason.to_lowercase().contains("top tier"), "{}", c.reason);
    }

    // ── build_replan_handoff ─────────────────────────────────────────────

    #[test]
    fn replan_resolution_builds_handoff_with_new_decomposition_instruction() {
        let handoff = build_replan_handoff(
            "scope mismatch: done_criteria requires editing files outside touched_files",
            "foo::bar",
            "diff --git a/x b/x",
            "sonnet",
            "the public API must live in crate root",
            "wire the thing",
        )
        .expect("scope-mismatch reason at sonnet should resolve to Replan");

        assert_eq!(handoff.classification.resolution, Resolution::Replan);

        let instr = handoff.instruction.to_lowercase();
        // Must explicitly ask for a NEW decomposition, not a re-run of the old one.
        assert!(
            instr.contains("do not") && instr.contains("re-run"),
            "instruction must explicitly forbid re-running the original decomposition: {}",
            handoff.instruction
        );
        assert!(
            instr.contains("original decomposition"),
            "instruction must name what NOT to repeat: {}",
            handoff.instruction
        );
        assert!(
            instr.contains("different approach"),
            "instruction must demand a different approach: {}",
            handoff.instruction
        );
        assert!(
            instr.contains("different scope"),
            "instruction must demand a different scope: {}",
            handoff.instruction
        );
        assert!(
            instr.contains("new decomposition") || instr.contains("brand new decomposition"),
            "instruction must demand a new decomposition: {}",
            handoff.instruction
        );

        // The failure_context is carried forward verbatim so the interpreter has it.
        assert_eq!(
            handoff.reason,
            "scope mismatch: done_criteria requires editing files outside touched_files"
        );
        assert_eq!(handoff.failed_tests, "foo::bar");
        assert_eq!(handoff.diff, "diff --git a/x b/x");
        assert_eq!(
            handoff.done_criteria,
            "the public API must live in crate root"
        );
        assert_eq!(handoff.task_summary, "wire the thing");
    }

    #[test]
    fn escalate_model_resolution_yields_no_handoff() {
        // An ordinary implementation bug below the top tier resolves to
        // EscalateModel — no handoff is built (there is nothing to replan
        // yet), and the caller gets the classification back via Err.
        let err = build_replan_handoff(
            "assertion `left == right` failed",
            "foo::bar",
            "diff --git a/x b/x",
            "sonnet",
            "some done_criteria",
            "some task",
        )
        .expect_err("ordinary implementation bug at sonnet should resolve to EscalateModel");
        assert_eq!(err.resolution, Resolution::EscalateModel);
    }

    #[test]
    fn top_tier_replan_handoff_also_builds() {
        // Already at the top tier and still failing → Replan, even without a
        // scope-mismatch-worded reason.
        let handoff =
            build_replan_handoff("assertion `left == right` failed", "", "", "opus", "", "")
                .expect("top-tier-still-failing should resolve to Replan");
        assert_eq!(handoff.classification.resolution, Resolution::Replan);
        assert!(handoff
            .instruction
            .to_lowercase()
            .contains("new decomposition"));
    }

    #[test]
    fn build_replan_handoff_never_panics_on_empty_input() {
        let result = build_replan_handoff("", "", "", "", "", "");
        // Empty model_tier is not top tier and empty reason has no
        // scope-mismatch marker → EscalateModel, but must not panic either way.
        assert!(result.is_err());
    }
}
