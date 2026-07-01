# harness-core

The single source of truth for the unchanging infrastructure shared across the
harness's Claude Code plugins. Anything that **must be identical in every
plugin** — the parallel-session-safe note store, the never-break-a-turn hook
wrapper, the `~/.claude/settings.json` install mechanics, per-project
addressing, the metrics sink — lives here once, so the plugins compose it
instead of re-implementing it (and drifting apart).

This is a **build-time LIBRARY crate, not a plugin**: it ships no `plugin.json`,
no hooks, and no binary. Each plugin links it statically into its self-contained
binary, so the distributed `crates/<plugin>/bin/` never references
`../harness-core` at runtime. Plugin-specific domain logic and config/metrics
*fields* stay in each plugin crate.

## What it provides

| Module | What it shares |
|---|---|
| `store` | Durable Obsidian-compatible note store, per-project, with parallel-session-safe fallback (a harness invariant) |
| `hook` | Hook stdin payload struct + `run_hook` wrapper that NEVER breaks a turn (exit 0 on any error/panic) |
| `hook_latency` | Central append-only Stop-hook latency ledger (`<base_dir>/state/hook-latency.jsonl`) — one shared file so aggregation is a single read; best-effort, never breaks a turn |
| `install` | `~/.claude/settings.json` load / timestamped backup / write + ownership detection by command markers |
| `hash` | FNV-1a (32/64-bit) — the single source of truth for the non-cryptographic hashes that are load-bearing for on-disk addressing |
| `projkey` | Per-project key `<basename>-<fnv1a32-hex>` — single source of truth for run-state file addressing |
| `config` | home/base-dir resolution, tilde expansion, env-var parsing primitives |
| `gate` | Shared run/runner/state gate machinery |
| `spans` | Span model + defensive JSONL loader (`~/.tracekit/<run_id>/spans.jsonl` on-disk contract) |
| `session` | Canonical per-session record (`<state_dir>/sessions/<id>.json`) |
| `usage` / `transcript` | Streaming JSONL transcript reader + per-model token/usage aggregation (never loads a whole transcript) |
| `metrics` | The append-only JSONL metrics SINK, parallel-safe |
| `pricing` | Model→USD cost table incl. cache read/write multipliers |
| `ledger` | Persistent daily spend ledger (`~/.budgetguard/state/ledger.json`) |
| `daily` | Once-per-calendar-day guard |
| `inject` | Shared substrate for context-injection hooks (`playbook`, `runbook`) |
| `inject_metrics` | Cross-harness UserPromptSubmit injection-size ledger, keyed by `turn_key = hash(session + prompt)` so the five injectors sum a single turn without cross-process coordination |
| `interrogate` | Domain-agnostic gate-by-gate interrogation control structure |
| `shell` | Cross-platform shell invocation, single source of truth |
| `trust` | Workspace-trust gate for honoring command strings from project-local config |

## Install (plugin)

Not applicable — harness-core is a dependency, not a plugin. Plugins depend on
it in their `Cargo.toml`; there are no hooks to wire and nothing to install.

## Build

```sh
cargo test
```

Built as part of the workspace (`cargo build --workspace --release`). It has no
committed `bin/` of its own — it is compiled into each plugin's binary.
