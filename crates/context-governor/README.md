# context-governor

A **thin control layer wrapped around Claude Code's built-in compaction**, wired as a single hook-dispatch binary. (日本語版: [README.ja.md](README.ja.md))

## Purpose

context-governor does not reinvent the context manager. It keeps the harness's existing compaction and adds only **four capabilities** around it:

- **pin** — keep norms (contracts, invariants, naming conventions, acceptance criteria) resident so they survive compaction.
- **lossless-recall** — move verbatim-required information to a backing store so it is not silently paraphrased away by summarization.
- **retrieval** — push large but situational reference bodies (exhaustive tables, full endpoint lists, appendices) out of the window and inject them only on the turns that need them.
- **tool-hygiene** — groom tool results, the biggest growth term in an agent loop, on every turn.

The design hinges on never conflating the **three axes** this layer touches:

- **size** (window occupancy) — actually shrink the window. Only three things do that: minimizing resident norm text, pushing reference bodies into retrieval, and grooming tool results per turn. Cache placement, pinning, and lowering the auto-compact threshold do **not** reduce size.
- **cost** (recompute / latency) — make prefill cheap by keeping the prompt cache warm. A stable prefix wins; rewriting the prefix every turn loses it.
- **correctness** (norm preservation) — stop norms and verbatim-required information from quietly disappearing inside a summary.

Crucially, **this layer never ships its own lossy summarizer** — compression is delegated to built-in compaction. context-governor only adds the discipline of *what stays resident / what is evicted / what is recalled later*. Each item lives in exactly one of three lanes — `Pinned` (always in the final context), `Verbatim` (never lossy-compressed), `Evictable` (groomable / evictable / retrievable) — and the lane is the single source of truth for how the item is handled. The "verbatim items must never be compressed" invariant is made *unrepresentable*: the only handler that compresses (`ToolResultGroomer`) accepts only `Evictable` tokens, so passing a `Pinned`/`Verbatim` token to it does not compile.

## How it works

context-governor is a **single hook-dispatch binary**. It reads the hook payload on stdin, branches on `hook_event_name`, runs the matching handler, and writes a JSON envelope to stdout. There are no slash commands; it is wired to Claude Code hooks (the one exception is the read-only `rollup` subcommand that aggregates the action ledger).

| Hook event | Handler | Role (axis) |
|---|---|---|
| `PostToolUse` | `ToolResultGroomer` | ★ primary size lever. Trims/summary-replaces bloated tool results. Handles `Evictable` only, so output is smaller than input. |
| `UserPromptSubmit` | `ContextInjector` | retrieval / reference-body injection. Adds `additionalContext` beside the prompt (a reduce before the model reads, not a replacement). |
| `SessionStart` | `StateRehydrator` | restore. Re-injects normative core / verbatim from the store so pins survive compaction (and reseeds on resume). |
| `PreCompact` | `CompactionGuard` | backstop. Snapshots the transcript before compaction and records verbatim spans to the backing store, then decides whether to proceed. Default is `Proceed`. |
| `Stop` / `SubagentStop` | `Checkpointer` | externalizes completed work to the backing store behind a threshold gate. **Side-effect only** — output is discarded and it never blocks. |

Two execution rules:

- **Never break a turn** — the whole dispatch runs inside `harness_core::hook::run_hook`, which swallows panics and exits 0. Empty/invalid payloads are a silent no-op (`{}`).
- **Only PreCompact may block** — a `Block` decision exits 2 (Claude Code's block signal). Every other path writes its envelope and exits 0.

## Measurement (action ledger)

The size axis must be *measured*, not merely asserted. The three size levers (groom / inject / snapshot) each append **one durable JSONL row** to `<state_dir>/ledger.jsonl` on every decision, carrying `saved_tokens` (window occupancy actually reclaimed) and `resident_tokens` (occupancy after the action), written via `harness_core::metrics::emit`.

```
context-governor rollup
```

aggregates the ledger into a deterministic, read-only view (`total_saved_tokens`, row count, per-action breakdown) — the evidence that lets you later disprove axis confusions like "pinning saves size".

Because it is a thin layer over built-in compaction, no extra API key is needed — it is **subscription-native** (hooks + binary).

## Coexistence with ctxrot

context-governor and [ctxrot](../ctxrot) register hooks on several of the same events but do not conflict — context-governor is the size/cost/correctness lever (groom/inject/rehydrate/guard/checkpoint) while ctxrot is the rot-detect/rescue/control lever. The per-event delegation (and why the two compose) is documented in [docs/coexistence-with-ctxrot.md](docs/coexistence-with-ctxrot.md).
