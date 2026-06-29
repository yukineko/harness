# autoflow

Session-end auto-flow gate for Claude Code — a single **Stop** hook that keeps a
session from ending with work left on the floor. When a turn finishes, autoflow
prompts `/record` once, then loops `/condukt` until the project's pending tasks
are cleared, and finally drains the cross-project backlog. It blocks the Stop
automatically up to 4× and, from the 5th prompt onward, asks the user before
continuing so a stuck loop can't run away.

Subscription-native: one hook plus a bundled Rust binary, **no API key**, no
daemon. The hook only ever emits a `block` decision with a reason — it never
runs work itself, and a missing state file or empty stdin exits 0 so the turn is
never broken.

## What it does

The Stop hook is a per-session state machine. Each phase decides whether to
block the turn (with a `/`-command nudge) or let it end:

| Phase | Condition | autoflow does |
|---|---|---|
| **Idle** | enough turns + tool events this session | block → `/session-insights:record` |
| **RecordRequested / Continuing** | condukt tasks still pending | block → `/condukt` (auto ≤4×, then ask from 5×) |
| **Continuing** | condukt clear, backlog has open items, compass charter fresh | block → `/backlog <next item>` |
| **Continuing** | backlog open but compass charter **stale** | nudge `/compass`, then stand down |
| **Done** | nothing pending | allow the turn to end |

compass is a soft dependency — if it's absent or unparseable, autoflow treats
the charter as fresh and proceeds. It also stands down entirely while another
live session holds the backlog lock, so a running `/flow` or `/backlog` driver
is never double-driven.

## Install (plugin)

Installed via the plugin marketplace, the bundled `hooks/hooks.json` wires the
**Stop** hook to `${CLAUDE_PLUGIN_ROOT}/bin/autoflow stop` automatically —
nothing else to do. Thresholds (min turns, min tool events, max backlog
prompts) come from config defaults; the gate is on by default.

## Standalone (cargo)

```sh
cargo install --path .
autoflow stop        # Stop hook: run the record→condukt→backlog state machine
```

`autoflow stop` reads the hook JSON on stdin and prints a `block` decision (or
nothing). `AUTOFLOW_DISABLE=1` silences the gate.

## Build

```sh
cargo test
```

The committed `bin/autoflow-*` binaries are what the plugin ships, so end users
need neither cargo nor an API key. Rebuild and recommit them (the workspace
builds with `cargo build --workspace --release`) when you change behavior the
hook relies on.
