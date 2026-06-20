# session-insights

Per-session **work metrics** for Claude Code. Two hooks roll up what actually
happened in a session — tool calls, turns, files touched — and derive a **size
class (XS–XL)** and a **work category** (coding / ops / research / mixed). View
it with `session-insights report`, or have each session logged as a dated note
in your **Obsidian vault**. Inspired by Devin's Session Insights, rebuilt as a
local, no-API-key hook.

Subscription-native: one bundled Rust binary, no daemon. Recording only writes
to its own state dir (and, opt-in, the vault) and always exits 0 — it never
blocks a turn.

## What it measures

| Hook | Records |
|---|---|
| **PostToolUse** | each tool call (per-tool counts, distinct files touched) |
| **Stop** | a completed turn; optionally writes the Obsidian session note |

From that it derives:
- **size**: XS / S / M / L / XL by total tool events (thresholds configurable)
- **category**: `coding` (Edit/Write), `ops` (Bash), `research` (Read/Grep/Web), or `mixed`
- per-session turns, tool events, file count, and top tools

## Report

```sh
session-insights report          # latest session
session-insights report --session <id-prefix>
session-insights report --all    # one line per recorded session
```

```
session a1b2c3d4  [2026-06-20T18:00:00+09:00]
  project: playbook
  size: L   category: coding
  turns: 12   tool events: 47   files: 9
  top tools: Edit 18, Bash 12, Read 9, Write 5, Grep 3
```

## Obsidian logging (opt-in)

Set `obsidian_log = true` and point `obsidian_vault` at your vault. On each Stop
the session is written/overwritten to `<vault>/sessions/<date>-<id>.md` with
frontmatter (`type: session`, size, category, turns…) — only if the vault dir
already exists (it never creates the vault).

## Install (plugin)

The bundled `hooks/hooks.json` wires both hooks automatically. Drop a
`session-insights.toml` to tune thresholds or enable Obsidian logging.

## Standalone (cargo)

```sh
cargo install --path .
session-insights install      # merge the PostToolUse + Stop hooks
session-insights report --all
session-insights status        # resolved config
session-insights uninstall
```

`install`/`uninstall` are idempotent, back up `settings.json`, and preserve
foreign hook groups.

## Build

```sh
make bins     # refresh bin/session-insights-darwin-<arch> and -linux-x86_64
cargo test
```

The committed `bin/session-insights-*` binaries are what the plugin ships, so end
users need neither cargo nor an API key.
