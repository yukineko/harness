//! interrogate — a domain-agnostic control structure for a human-driven,
//! gate-by-gate "interrogation" of gathered material.
//!
//! Two future plugins (compass, specforge) parameterize this with their own
//! rigor gates; this module deliberately knows nothing about either, nor about
//! specs/charters/requirements. It carries no domain wording.
//!
//! ARCHITECTURE: the interrogation loop is NOT self-driven by Rust. The thing
//! that asks the human (`AskUserQuestion`) is a Claude Code tool that a binary
//! cannot call. So this module is pure and stateless-style: it exposes only
//!   - [`evaluate`] — run the gates over a [`Bundle`] to surface [`OpenQuestion`]s, and
//!   - [`apply`] — fold one human [`Answer`] into a [`CarveState`] and re-evaluate.
//!
//! An external SKILL (the LLM) drives the loop:
//!   `evaluate` → (LLM asks the human) → `apply(answer)` → repeat,
//! while the calling binary persists [`CarveState`] across invocations.
//!
//! Purity invariant: no file I/O, no network, no LLM, no `AskUserQuestion`, no
//! question-text generation. [`Authority`] is carried as data — a presentation
//! HINT for the SKILL — and is never used to auto-resolve conflicts in code.

use serde::{Deserialize, Serialize};

/// Provenance strength of a piece of gathered material. Used only as a tiebreak
/// HINT when presenting defaults to a human — never to auto-resolve conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Authority {
    High,
    Mid,
    Low,
}

/// A single piece of gathered material.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fragment {
    pub text: String,
    pub source_path: String,
    pub authority: Authority,
    /// Relevance — orthogonal to `authority`.
    pub score: i64,
    pub anchor: Option<String>,
}

/// A collection of gathered fragments. Resolved human answers are folded back in
/// here (by [`apply`]) as high-authority fragments, so the next [`evaluate`]
/// reflects the decisions already made.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Bundle {
    pub fragments: Vec<Fragment>,
}

/// One unmet rigor point, normalized. `default` is the tiebreak-hint value
/// (highest-authority candidate) that a SKILL should present as the first
/// "recommended" option; `None` when there is no defensible default.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenQuestion {
    /// e.g. "C2" / "G3" — the domain supplies the label.
    pub gate: String,
    /// What this question is about (a requirement / charter-field ref).
    pub reference: String,
    /// Machine-described gap — NOT the human-facing wording.
    pub gap: String,
    pub sources: Vec<Fragment>,
    pub default: Option<String>,
}

/// A human answer fed back in. The module records it; it does not interpret the
/// wording.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Answer {
    pub gate: String,
    pub reference: String,
    /// Chosen / typed answer; becomes an [`Authority::High`] fragment.
    pub value: String,
    /// `true` => the human chose to defer => drop the remaining points to a
    /// sentinel rather than keep asking.
    pub defer: bool,
}

/// Terminal vs. continuing state of the interrogation.
/// `Sentinel` means `max_rounds` was hit or the human deferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CarveStatus {
    Open,
    Resolved,
    Sentinel,
}

/// Serializable state the calling binary persists across invocations. Rust never
/// loops over this; the driving SKILL advances it one [`apply`] at a time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarveState {
    pub bundle: Bundle,
    pub round: u32,
    /// `0` => synchronous interrogation disabled: the first [`apply`] that still
    /// finds open questions immediately yields [`CarveStatus::Sentinel`].
    pub max_rounds: u32,
    pub answers: Vec<Answer>,
    pub status: CarveStatus,
}

impl CarveState {
    /// Fresh state over `bundle`: round 0, no answers, status [`CarveStatus::Open`].
    pub fn new(bundle: Bundle, max_rounds: u32) -> Self {
        CarveState {
            bundle,
            round: 0,
            max_rounds,
            answers: Vec::new(),
            status: CarveStatus::Open,
        }
    }
}

/// Domain supplies the gate set. Pure: detect unmet points from the current
/// bundle. Implementations MUST NOT perform I/O or auto-resolve via authority.
pub trait RigorGates {
    fn evaluate(&self, bundle: &Bundle) -> Vec<OpenQuestion>;
}

/// Stateless evaluation: run the gates against a bundle, return the unmet open
/// questions. A thin pass-through that fixes the public entry point so callers
/// don't depend on the trait method directly.
pub fn evaluate<G: RigorGates>(gates: &G, bundle: &Bundle) -> Vec<OpenQuestion> {
    gates.evaluate(bundle)
}

/// Fold one answer into `state`, then re-evaluate. Semantics:
///  - record the answer; if `!defer`, append it to `bundle.fragments` as an
///    [`Authority::High`] fragment (so re-evaluation reflects the human's
///    resolution — decisions become the next high-authority source).
///  - increment `round`.
///  - re-run the gates on the updated bundle to get the remaining open questions.
///  - set status: [`CarveStatus::Resolved`] if none remain;
///    [`CarveStatus::Sentinel`] if `answer.defer` OR `round >= max_rounds`
///    (and `max_rounds == 0` means any remaining open question => `Sentinel`
///    immediately); otherwise [`CarveStatus::Open`].
///  - return the remaining open questions.
pub fn apply<G: RigorGates>(
    gates: &G,
    state: &mut CarveState,
    answer: Answer,
) -> Vec<OpenQuestion> {
    let deferred = answer.defer;

    // Resolved (non-deferred) decisions become the next high-authority source.
    if !deferred {
        state.bundle.fragments.push(Fragment {
            text: answer.value.clone(),
            source_path: format!("interrogate:answer:{}:{}", answer.gate, answer.reference),
            authority: Authority::High,
            score: 0,
            anchor: None,
        });
    }

    state.answers.push(answer);
    state.round = state.round.saturating_add(1);

    let remaining = gates.evaluate(&state.bundle);

    state.status = if remaining.is_empty() {
        CarveStatus::Resolved
    } else if deferred || state.round >= state.max_rounds {
        // `max_rounds == 0` collapses here on the first apply (round becomes 1,
        // 1 >= 0), surfacing the sentinel immediately when work still remains.
        CarveStatus::Sentinel
    } else {
        CarveStatus::Open
    };

    remaining
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dummy domain gates: a single point stays open until the bundle contains a
    /// fragment whose text equals `needle`.
    struct UntilText {
        needle: String,
    }

    impl RigorGates for UntilText {
        fn evaluate(&self, bundle: &Bundle) -> Vec<OpenQuestion> {
            if bundle.fragments.iter().any(|f| f.text == self.needle) {
                Vec::new()
            } else {
                vec![OpenQuestion {
                    gate: "G1".to_string(),
                    reference: "ref-1".to_string(),
                    gap: "needle missing".to_string(),
                    sources: Vec::new(),
                    default: Some(self.needle.clone()),
                }]
            }
        }
    }

    fn gates() -> UntilText {
        UntilText {
            needle: "DONE".to_string(),
        }
    }

    fn answer(value: &str, defer: bool) -> Answer {
        Answer {
            gate: "G1".to_string(),
            reference: "ref-1".to_string(),
            value: value.to_string(),
            defer,
        }
    }

    /// (a) evaluate returns the dummy's open questions.
    #[test]
    fn evaluate_surfaces_open_questions() {
        let open = evaluate(&gates(), &Bundle::default());
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].gate, "G1");
        assert_eq!(open[0].default.as_deref(), Some("DONE"));

        // Once the needle is present, nothing is open.
        let mut b = Bundle::default();
        b.fragments.push(Fragment {
            text: "DONE".to_string(),
            source_path: "x".to_string(),
            authority: Authority::Mid,
            score: 0,
            anchor: None,
        });
        assert!(evaluate(&gates(), &b).is_empty());
    }

    /// (b) a resolving answer appends a High fragment, bumps round, reaches Resolved.
    #[test]
    fn apply_resolves_and_records_high_fragment() {
        let mut state = CarveState::new(Bundle::default(), 3);
        let remaining = apply(&gates(), &mut state, answer("DONE", false));

        assert!(remaining.is_empty());
        assert_eq!(state.status, CarveStatus::Resolved);
        assert_eq!(state.round, 1);
        assert_eq!(state.answers.len(), 1);

        // The decision was folded back in as a High-authority fragment.
        let last = state.bundle.fragments.last().unwrap();
        assert_eq!(last.text, "DONE");
        assert_eq!(last.authority, Authority::High);
    }

    /// (c) apply reaches Sentinel when round >= max_rounds with work remaining.
    #[test]
    fn apply_hits_sentinel_at_max_rounds() {
        let mut state = CarveState::new(Bundle::default(), 1);
        // A non-resolving answer ("nope" != needle) still leaves the point open;
        // round becomes 1 == max_rounds => Sentinel.
        let remaining = apply(&gates(), &mut state, answer("nope", false));

        assert_eq!(remaining.len(), 1);
        assert_eq!(state.round, 1);
        assert_eq!(state.status, CarveStatus::Sentinel);
    }

    /// (d) max_rounds == 0 with a remaining open question => Sentinel on first apply.
    #[test]
    fn apply_max_rounds_zero_sentinels_immediately() {
        let mut state = CarveState::new(Bundle::default(), 0);
        let remaining = apply(&gates(), &mut state, answer("nope", false));

        assert_eq!(remaining.len(), 1);
        assert_eq!(state.round, 1);
        assert_eq!(state.status, CarveStatus::Sentinel);
    }

    /// (e) apply with defer: true => Sentinel, and the deferred value is NOT
    /// folded into the bundle as a resolving fragment.
    #[test]
    fn apply_defer_sentinels_without_resolving() {
        let mut state = CarveState::new(Bundle::default(), 5);
        let before = state.bundle.fragments.len();
        let remaining = apply(&gates(), &mut state, answer("whatever", true));

        assert_eq!(state.status, CarveStatus::Sentinel);
        assert_eq!(remaining.len(), 1);
        assert_eq!(state.round, 1);
        // Deferred answers are recorded but not appended as fragments.
        assert_eq!(state.answers.len(), 1);
        assert_eq!(state.bundle.fragments.len(), before);
    }
}
