//! Integration tests for the context-governor contract.
//!
//! **Phase 1** tests (below the fold) pin the *shape* of the frozen contract
//! and the two type-level invariants (I1 lane intent, I2 unrepresentable
//! lossy-compress of Verbatim/Pinned).
//!
//! **Phase 2** tests (added here) exercise the real `DefaultClassifier` and
//! `DefaultInjector` handlers end-to-end via the public crate API:
//! - Classifier round-trip: heading-based section split, SpecClass/Lane parity,
//!   `check_resident` Ok / Overrun paths (I3).
//! - Injector with a real file: env-var wiring, heading-match injection, and
//!   ToC sentinel on irrelevant prompts.

use context_governor::defaults::{DefaultClassifier, DefaultInjector};
use context_governor::{
    Action, ContextItem, Evictable, HookInput, HookOutput, ItemBody, ItemId, Lane, Overrun,
    SpecClass, StandingBudget, StoreKey,
};
use context_governor::{ContextInjector, SpecClassifier};

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

// ── Phase 2: DefaultClassifier integration ────────────────────────────────────

/// Spec doc used across the Phase 2 classifier / injector integration tests.
const PHASE2_DOC: &str = "\
# Rules
All API calls MUST include an Authorization header.
Retry on 503 up to 3 times with exponential back-off.

# Reference Table
| Field  | Type   | Required |
|--------|--------|----------|
| id     | uint64 | yes      |
| name   | string | yes      |

# Examples
```bash
curl -H 'Authorization: Bearer TOKEN' https://api.example.com/users
```
";

/// Phase 2 — classifier round-trip: the classifier splits the doc into sections
/// with the correct SpecClass/Lane parity (NormativeCore → Pinned, ReferenceBody
/// → Evictable) and returns them in document order.
#[test]
fn classifier_round_trip_spec_class_and_lane() {
    let items = DefaultClassifier.classify(PHASE2_DOC);
    // 3 sections: "Rules" (normative), "Reference Table" (reference), "Examples" (reference).
    assert_eq!(items.len(), 3, "expected 3 sections from PHASE2_DOC");

    let (class0, item0) = &items[0];
    assert_eq!(
        *class0,
        SpecClass::NormativeCore,
        "section 0 should be NormativeCore"
    );
    assert_eq!(item0.lane, Lane::Pinned, "NormativeCore must be Pinned");
    assert!(
        item0.tokens > 0,
        "non-empty section must have positive token estimate"
    );

    let (class1, item1) = &items[1];
    assert_eq!(
        *class1,
        SpecClass::ReferenceBody,
        "section 1 (Reference Table) should be ReferenceBody"
    );
    assert_eq!(
        item1.lane,
        Lane::Evictable,
        "ReferenceBody must be Evictable"
    );

    let (class2, item2) = &items[2];
    assert_eq!(
        *class2,
        SpecClass::ReferenceBody,
        "section 2 (Examples) should be ReferenceBody"
    );
    assert_eq!(
        item2.lane,
        Lane::Evictable,
        "ReferenceBody must be Evictable"
    );
}

/// Phase 2 — I3: `check_resident` returns `Ok` when Pinned tokens fit the
/// budget and `Err(Overrun)` with correct arithmetic when they exceed it.
#[test]
fn classifier_check_resident_ok_and_overrun() {
    let context_items: Vec<ContextItem> = DefaultClassifier
        .classify(PHASE2_DOC)
        .into_iter()
        .map(|(_, i)| i)
        .collect();

    // Generous budget → Ok.
    let ok_budget = StandingBudget {
        max_resident_tokens: 100_000,
    };
    assert!(
        DefaultClassifier
            .check_resident(&context_items, &ok_budget)
            .is_ok(),
        "should be Ok with a generous budget"
    );

    // Tiny budget → Err(Overrun) with correct excess().
    let tiny_budget = StandingBudget {
        max_resident_tokens: 1,
    };
    let err = DefaultClassifier
        .check_resident(&context_items, &tiny_budget)
        .expect_err("should return Overrun when budget is exceeded");
    assert!(
        err.resident_tokens > tiny_budget.max_resident_tokens,
        "resident_tokens must exceed max_resident_tokens"
    );
    assert_eq!(
        err.excess(),
        err.resident_tokens.saturating_sub(err.max_resident_tokens),
        "excess() must equal the overage amount"
    );
}

// ── Phase 2: DefaultInjector integration ─────────────────────────────────────

/// Phase 2 — injector with a real file: when `CONTEXT_GOVERNOR_REFERENCE_DOC`
/// points to a valid file, `inject` reads it and returns an `additionalContext`
/// envelope. Tests both the heading-match path and the fallback-to-ToC path.
#[test]
fn injector_reads_reference_doc_from_env_and_injects() {
    use std::io::Write as _;

    let mut tmp = tempfile::NamedTempFile::new().expect("temp file");
    tmp.write_all(PHASE2_DOC.as_bytes()).expect("write doc");
    tmp.flush().expect("flush");

    std::env::set_var("CONTEXT_GOVERNOR_REFERENCE_DOC", tmp.path());

    // Heading-match path: "examples" overlaps with the "Examples" heading.
    let input_match: HookInput = serde_json::from_value(serde_json::json!({
        "session_id": "integ",
        "transcript_path": "",
        "cwd": "",
        "hook_event_name": "UserPromptSubmit",
        "prompt": "show me some examples",
    }))
    .unwrap();
    let out_match = DefaultInjector.inject(&input_match);
    let json_match = out_match.to_json();
    assert!(
        json_match.contains("\"additionalContext\""),
        "heading-match path must produce additionalContext; got: {json_match}"
    );
    assert!(
        json_match.contains("\"hookEventName\":\"UserPromptSubmit\""),
        "hookEventName must be UserPromptSubmit; got: {json_match}"
    );

    // ToC path: "butterfly" has no overlap with any heading.
    let input_toc: HookInput = serde_json::from_value(serde_json::json!({
        "session_id": "integ",
        "transcript_path": "",
        "cwd": "",
        "hook_event_name": "UserPromptSubmit",
        "prompt": "what is a butterfly?",
    }))
    .unwrap();
    let out_toc = DefaultInjector.inject(&input_toc);
    let json_toc = out_toc.to_json();
    assert!(
        json_toc.contains("Reference sections:"),
        "ToC path must list reference sections; got: {json_toc}"
    );

    // Clean up.
    std::env::remove_var("CONTEXT_GOVERNOR_REFERENCE_DOC");
}
