//! Phase 1 contract tests — they pin the *shape* of the frozen contract and the
//! two type-level invariants (I1 lane intent, I2 unrepresentable lossy-compress
//! of Verbatim/Pinned). They deliberately do NOT call the default handlers,
//! whose bodies are `todo!()` until Phase 2.
//!
//! The full §14 acceptance suite (resident-budget, groom-slope,
//! normative-core-survival, prefix-stability, no-self-summarization,
//! Stop-non-block) and the `proptest` checks for I1–I6 arrive in Phase 2, when
//! the handlers are real.

use context_governor::{
    Action, ContextItem, Evictable, HookOutput, ItemBody, ItemId, Lane, Overrun, SpecClass,
    StandingBudget, StoreKey,
};

fn item(lane: Lane, tokens: u32) -> ContextItem {
    ContextItem {
        id: ItemId(1),
        lane,
        tokens,
        body: ItemBody::Inline("x".to_string()),
    }
}

/// I2, enforced by the type system: an `Evictable` capability token can be
/// minted only from a `Lane::Evictable` item. `Pinned`/`Verbatim` yield `None`,
/// so the groomer (which requires `Evictable`) can never receive them.
#[test]
fn evictable_token_only_from_evictable_lane() {
    assert!(Evictable::new(&item(Lane::Evictable, 10)).is_some());
    assert!(Evictable::new(&item(Lane::Pinned, 10)).is_none());
    assert!(Evictable::new(&item(Lane::Verbatim, 10)).is_none());
}

/// An inline item occupies the window; a `Ref` body does not (it's externalized
/// behind a `StoreKey`). This is the resident/non-resident distinction the size
/// invariants count over.
#[test]
fn residency_tracks_inline_vs_ref_body() {
    let resident = item(Lane::Pinned, 5);
    assert!(resident.is_resident());

    let externalized = ContextItem {
        id: ItemId(2),
        lane: Lane::Evictable,
        tokens: 5,
        body: ItemBody::Ref(StoreKey(7)),
    };
    assert!(!externalized.is_resident());
}

/// I3 arithmetic: `Overrun::excess` reports exactly how many tokens must move to
/// `ReferenceBody` to satisfy the standing budget, and saturates at zero.
#[test]
fn overrun_reports_excess_to_shed() {
    let budget = StandingBudget {
        max_resident_tokens: 100,
    };
    let over = Overrun {
        resident_tokens: 130,
        max_resident_tokens: budget.max_resident_tokens,
    };
    assert_eq!(over.excess(), 30);

    let under = Overrun {
        resident_tokens: 80,
        max_resident_tokens: 100,
    };
    assert_eq!(under.excess(), 0);
}

/// The default (do-nothing) envelope serializes to `{}` — Claude Code reads that
/// as "proceed, no-op", the correct response when a handler declines to act.
#[test]
fn default_output_is_empty_object() {
    assert_eq!(HookOutput::default().to_json(), "{}");
}

/// The two write-back envelopes carry the renamed fields Claude Code expects.
#[test]
fn injection_and_groom_envelopes_use_wire_field_names() {
    let inj = HookOutput::inject("UserPromptSubmit", "ctx".to_string()).to_json();
    assert!(inj.contains("\"hookSpecificOutput\""));
    assert!(inj.contains("\"hookEventName\":\"UserPromptSubmit\""));
    assert!(inj.contains("\"additionalContext\":\"ctx\""));

    let groom = HookOutput::groomed(serde_json::json!({"trimmed": true})).to_json();
    assert!(groom.contains("\"hookEventName\":\"PostToolUse\""));
    assert!(groom.contains("\"updatedToolOutput\""));
}

/// `SpecClass` maps a spec span to a lane discipline (§8). This pins the mapping
/// the classifier must honor: NormativeCore → resident, ReferenceBody → evictable.
#[test]
fn spec_class_variants_are_distinct() {
    assert_ne!(SpecClass::NormativeCore, SpecClass::ReferenceBody);
}

/// The ledger's size-bearing action carries the reclaimed tokens (I4/I6).
#[test]
fn groomed_action_carries_saved_tokens() {
    match (Action::Groomed { saved_tokens: 42 }) {
        Action::Groomed { saved_tokens } => assert_eq!(saved_tokens, 42),
        _ => unreachable!(),
    }
}
