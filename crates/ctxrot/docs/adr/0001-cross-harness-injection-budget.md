# ADR 0001 — Cross-harness injection budget

- **Status:** Proposed (observation step landed in ctxrot; cross-harness aggregation + shared budget pending)
- **Date:** 2026-06-22
- **Scope:** the harness-plugin family (`ctxrot`, `playbook`, `gauge`, `session-insights`, …) — multiple plugins, distributed together from the `yukineko/claude-harnesses` monorepo
- **Owner:** ctxrot maintainers (originating repo); requires buy-in from the sibling plugins

## Context

ctxrot fights context rot by keeping the window light. But it is not the only
plugin injecting into the model context, and **several plugins inject on the same
event**: `UserPromptSubmit`. ctxrot's `guard` and `playbook` both add text every
qualifying prompt; `gauge` / `session-insights` add their own. Each is individually
"conditional and minimal", but **nobody bounds the sum**.

The failure mode is the same one ctxrot exists to prevent, one level up: a family
of well-behaved plugins that each add "just a little" every turn collectively
push the window the wrong way. ctxrot capping *its own* injection (per
`guard_inject_max_chars`, N2) is necessary but not sufficient — it cannot see, let
alone bound, what its siblings add in the same turn.

This is intrinsically cross-repo: no single plugin can solve it alone, because the
budget must be shared state across processes that don't otherwise coordinate.

## Constraints (inherited from ctxrot's invariants)

1. A hook must never break the turn (exit 0, silent on any error). Any shared
   mechanism must degrade to "inject normally" when unavailable.
2. No LLM calls in hooks.
3. Per-turn coordination must be cheap (hooks run on a tight timeout) and must not
   require a daemon.
4. CJK-safe accounting (char counts, not bytes) where a budget is enforced.
5. Subscription-only; no external services.

## Decision

Proceed in two phases. **Observe before coordinating.**

### Phase 1 — Observation (this ADR lands the ctxrot half)

Each injecting plugin records *its own* per-turn injection size to a place a
report can sum. ctxrot now emits an `inject` metric per qualifying prompt
(`{chars, blocks}`, post-cap) to `<state_dir>/metrics.jsonl`, rolled up as
`inject_chars` and shown in `ctxrot metrics compare` (the `inject A/B` line).

The cross-harness rollup belongs in an aggregator that already reads multiple
plugins' signals — **`gauge` or `session-insights`** — which should:
- read each plugin's per-turn injection size (from a shared, agreed channel; see
  below), and
- report combined per-turn and per-session injection, ideally with a per-plugin
  breakdown.

Deliverable of Phase 1: you can *see* the combined number. No enforcement yet.

### Phase 2 — Shared budget (design only; implement after Phase 1 data)

Two candidate mechanisms, to be chosen once Phase 1 shows the real magnitude:

- **(A) Loose cooperative budget via a per-turn shared state file.** A well-known
  path (e.g. `~/.claude/state/inject-budget-<turn-id>.json`) holds "remaining
  injection budget for this turn". Each `UserPromptSubmit` hook, in registration
  order, reads remaining, injects up to its share, decrements, writes back. The
  turn id comes from the hook input (session id + a per-turn nonce). Pros: no
  daemon, fully degradable (missing/locked file → inject normally). Cons: hook
  ordering is not guaranteed across plugins; file contention; defining "a turn"
  reliably across hooks.

- **(B) Priority-ranked global cap.** A shared config declares a family-wide
  per-turn ceiling and a priority order across plugins (safety > knowledge
  injection > anchor/supplemental). Each plugin tags its blocks with a priority
  (ctxrot already does internally — `Prio` in `guard.rs`); the aggregator (or a
  thin shared lib) trims lowest-priority blocks across *all* plugins to fit. Pros:
  globally optimal drops, reuses ctxrot's existing priority model. Cons: needs a
  shared library or a coordinating hook all plugins call; the largest change.

**Leaning:** start with (A) as an opt-in, default-off cooperative budget (matches
ctxrot's conservative posture and the "never break the turn" invariant), and only
escalate to (B) if Phase 1 data shows the loose scheme leaves too much on the
table. Either way, the shared channel and "turn id" definition are the hard part
and must be specified in a follow-up ADR before code.

## Options considered (and rejected for now)

- **Do nothing / rely on each plugin's own cap.** Rejected: per-plugin caps
  cannot bound the sum, which is the actual problem.
- **A coordinating daemon.** Rejected: violates the no-daemon / cheap-hook
  constraint and adds an availability dependency that could break turns.
- **Hard global cap with no priority.** Rejected: would drop safety-critical
  warnings as readily as supplemental anchors.

## Consequences

- ctxrot's injection is now observable in-repo (Phase 1, ctxrot side). This is the
  seed; the family-wide number requires the sibling plugins to emit comparably and
  an aggregator to sum.
- No behavior change yet for users: `inject` is a metric only.
- A second ADR is required to fix the shared-channel format and the turn-id
  definition before any enforcement code in the sibling repos.
- The work is sequenced after ctxrot's own cap (N2) deliberately: bounding the
  largest single contributor first makes the residual cross-harness problem
  smaller and easier to measure.

## Outcome verification

How we will know each phase actually worked (not just merged):

- **Phase 1 (observation):**
  - `ctxrot metrics compare on- off-` shows a non-zero `inject` line for guard-ON
    sessions and ~0 for `GUARD_DISABLE` sessions. *(verifiable now, in this repo)*
  - The chosen aggregator (`gauge`/`session-insights`) reports a combined per-turn
    injection figure across ≥2 plugins on a real session, with a per-plugin
    breakdown that sums to the total. *(pending those repos)*
- **Phase 2 (budget), once implemented:**
  - With the budget enabled and several injecting plugins active, the combined
    per-turn injection stays at or below the configured ceiling on a heavy session
    (measured via the Phase 1 report).
  - Disabling the budget (or removing the shared state file) reproduces the
    uncapped combined figure — i.e. the mechanism, not chance, did the bounding.
  - No turn is ever broken by the coordination path: inject-normally fallback is
    exercised by a test that makes the shared state unreadable/locked.

## References

- ctxrot N2 (`guard_inject_max_chars`) — per-plugin cap and the `Prio` model this
  ADR's option (B) would lift to family scope: `src/hooks/guard.rs`.
- ctxrot `inject` metric (Phase 1 observation): `src/metrics.rs`, `src/hooks/guard.rs`.
