# ctxrot

A **context-rot guard for Claude Code**, written in Rust.

In long sessions the model's attention degrades: early instructions get buried,
decisions and open todos sink, raw dumps dilute everything (*context rot*).
`ctxrot` is a single binary that hooks into Claude Code to **detect, rescue,
restore, and distill** conversation context.

## What it does

Each hook is one subcommand of the `ctxrot` binary. It reads the hook's JSON
payload on **stdin**. Cardinal rule: a hook **never breaks your turn** — on any
error it exits 0 and stays silent.

| Subcommand | Hook | What it does |
|---|---|---|
| `ctxrot guard` | `UserPromptSubmit` | Detects large refs (big local files / URLs / "全文" keywords) and **context-budget bands** (50/75/90% of the window). Injects *minimal, conditional* advice — only when something is relevant, and budget advice only once per band crossing (so the advice itself doesn't cause rot). |
| `ctxrot rescue` | `PreCompact` | Right before `/compact`, streams the recent transcript and writes a durable **rescue note** (decisions, open todos, touched files, links, raw recent turns) so nothing is lost to lossy compaction. Deterministic, no LLM. |
| `ctxrot restore` | `SessionStart` | At session start, injects a **compact carryover** (decisions + open todos + a link) from the latest note — never the whole note. |
| `ctxrot toolguard` | `PostToolUse` | When a `Read`/`Bash`/`Grep`/… returns a huge payload, nudges you to route the *next* heavy read through a sub-agent and keep only conclusions. |

Plus the **`/distill` skill** for on-demand, high-quality LLM distillation (the
hooks are the cheap deterministic safety net; `/distill` is the smart one).

### Design split

- **Hooks = fast, deterministic, no LLM.** Safe inside PreCompact's tight timeout.
- **`/distill` skill = LLM-quality summarization on demand**, run inside the
  session (can delegate heavy reads to sub-agents via `Task`).

## Build

```sh
cargo build --release
# binary at target/release/ctxrot
```

## Install

```sh
# 1. create default config + store dirs
ctxrot init

# 2. preview the settings.json changes
ctxrot install --dry-run

# 3. apply (backs up ~/.claude/settings.json first)
ctxrot install
```

`install` is idempotent and **replaces** any prior ctxrot entries and the legacy
`context-rot-guard.py` hook, while preserving your other hooks and settings.
Remove with `ctxrot uninstall`.

For the `/distill` skill:

```sh
cp skills/distill.md ~/.claude/commands/distill.md
```

## Configuration

`~/.ctxrot/config.toml` (created by `ctxrot init`):

```toml
store_dir = "~/.ctxrot/store"   # can point at an Obsidian vault
state_dir = "~/.ctxrot/state"
context_window = 200000
large_file_bytes = 50000
huge_tool_output_bytes = 50000
bands = [0.50, 0.75, 0.90]
```

Env overrides (Python v1 compatibility): `GUARD_DISABLE` (any value → no-op),
`CLAUDE_CONTEXT_WINDOW`, `GUARD_LARGE_FILE_BYTES`.

## Store

Notes are Obsidian-compatible markdown, grouped per project (keyed by cwd):
`<store_dir>/<project-basename>-<hash>/`. Inspect with:

```sh
ctxrot note list      # newest first
ctxrot note latest    # path of the most recent note
ctxrot note dir       # the project's note directory
```

## How memory survives a session

```
… long session …
   │  guard:    "推定 ~76% — /distill で退避を"   (UserPromptSubmit, once per band)
   │  toolguard:"Read が ~59KB 投入 — 次回は sub-agent 経由"
   ▼
/compact ──► rescue (PreCompact): writes rescue-<ts>.md   ← nothing lost
   ▼
new session ──► restore (SessionStart): injects decisions + todos + link
```

## Development

```sh
cargo test          # unit + fixture tests
cargo build
```

Manual hook check:

```sh
echo '{"prompt":"read /big.log","cwd":"'"$PWD"'","transcript_path":"tests/fixtures/transcript.jsonl","session_id":"s1"}' | ctxrot guard
```

## License

MIT
