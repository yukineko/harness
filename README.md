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
| `ctxrot guard` | `UserPromptSubmit` | Detects large refs (big local files / URLs / "全文" keywords) and **context-budget bands** (50/75/90% of the window). Injects *minimal, conditional* advice — only when something is relevant, and budget advice only once per band crossing (so the advice itself doesn't cause rot). At **band ≥ 2 (~75%+)** it also **preemptively writes a rescue note** (same format as below), so a manual `/compact` *or `/clear`* is safe without waiting for PreCompact. |
| `ctxrot rescue` | `PreCompact` | Right before `/compact`, streams the recent transcript and writes a durable **rescue note** (decisions, open todos, touched files, links, raw recent turns) so nothing is lost to lossy compaction. Deterministic, no LLM. The note filename carries a **session tag** (`rescue-<session>-<ts>.md`). Same writer also powers guard's preemptive rescue (labeled `trigger: band-NN%`). |
| `ctxrot restore` | `SessionStart` | At session start, injects a **compact carryover** (decisions + open todos + a link). With **parallel sessions** sharing one project dir, it prefers *this* session's own note (matched by session tag) and only falls back to the project-wide latest — never the whole note. |
| `ctxrot preguard` | `PreToolUse` | **Preventive gate, before the load.** An *unbounded* `Read` (no `limit`) of a local file at/above `gate_file_bytes` (default **1MB**) is **denied** with an actionable reason — route it through a sub-agent or re-`Read` a bounded slice. Narrow by design (only `Read`, only huge files, `limit` always bypasses) so normal source reads are untouched. Set `gate_file_bytes = 0` to disable. |
| `ctxrot toolguard` | `PostToolUse` | When a `Read`/`Bash`/`Grep`/… returns a huge payload, nudges you to route the *next* heavy read through a sub-agent and keep only conclusions. (Handles the 50KB–1MB middle band the `preguard` gate lets through.) |

Plus the **`/distill` skill** for on-demand, high-quality LLM distillation (the
hooks are the cheap deterministic safety net; `/distill` is the smart one).

### Design split

- **Hooks = fast, deterministic, no LLM.** Safe inside PreCompact's tight timeout.
- **`/distill` skill = LLM-quality summarization on demand**, run inside the
  session (can delegate heavy reads to sub-agents via `Task`).

## Install (recommended: as a Claude Code plugin)

This repo is **both the Rust crate and a Claude Code plugin/marketplace**. The
plugin bundles the five hooks, the `/distill` skill, the `ctxrot-distiller`
subagent, and a prebuilt binary (`bin/ctxrot`) — so installs run entirely on your
Claude **subscription**, no API key and no separate `cargo install` needed.

```text
# in Claude Code:
/plugin marketplace add <git-url-of-this-repo>
/plugin install ctxrot@yukineko
```

Hooks call `${CLAUDE_PLUGIN_ROOT}/bin/ctxrot <sub>`. `bin/ctxrot` is a small POSIX
launcher that picks the right per-platform binary (`bin/ctxrot-<os>-<arch>`) for
the host, so the same repo works on Linux and macOS. Run `ctxrot init` once for
the config + store dirs (optional; defaults work without it).

It runs entirely on your Claude subscription — the hooks and subagent execute in
the normal session model, no `ANTHROPIC_API_KEY` and no separate `cargo install`.

> **Per-user step:** each user must `/plugin marketplace add <git-url>` once
> (Claude Code does not auto-register marketplaces from a checked-in repo).
> Committing `.claude/settings.json` with `enabledPlugins` can pin *enabling*, but
> not the marketplace registration.

### Alternative: manual install (no plugin)

```sh
cargo build --release
ctxrot init                 # config + store dirs
ctxrot install --dry-run    # preview ~/.claude/settings.json changes
ctxrot install              # apply (backs up settings.json first)
cp -r skills/distill ~/.claude/skills/   # the /distill skill
```

`ctxrot install` is idempotent and **replaces** any prior ctxrot entries and the
legacy `context-rot-guard.py` hook, while preserving your other hooks and
settings. Remove with `ctxrot uninstall`.

## Platform support / building the binaries

The plugin ships prebuilt per-platform binaries, selected at runtime by the
`bin/ctxrot` launcher:

| Host | File | Status |
|---|---|---|
| Linux x86_64 | `bin/ctxrot-linux-x86_64` | bundled |
| macOS Apple Silicon | `bin/ctxrot-darwin-arm64` | build on a Mac (see below) |
| macOS Intel | `bin/ctxrot-darwin-x86_64` | build on a Mac (see below) |

If a host has no matching binary, the launcher exits 0 silently (a hook never
breaks your turn) and prints a one-line build hint to stderr.

**Build for your platform** — run on that machine and commit the result:

```sh
# host platform (Linux here, Apple Silicon on a Mac, etc.)
scripts/build-plugin-bin.sh

# cross-target on a Mac to also produce the Intel build:
rustup target add x86_64-apple-darwin
scripts/build-plugin-bin.sh x86_64-apple-darwin

git add bin/ && git update-index --chmod=+x bin/ctxrot bin/ctxrot-*
git commit -m "Add <platform> binary"
```

The script normalizes the Rust host triple to `ctxrot-<os>-<arch>`. Because this
repo lives on a `core.filemode=false` mount, exec bits are forced into the git
index with `git update-index --chmod=+x` (otherwise the launcher/binaries would
check out non-executable and hooks would fail).

## Plugin layout

```
.claude-plugin/plugin.json        # plugin manifest
.claude-plugin/marketplace.json   # single-plugin marketplace (name: yukineko)
hooks/hooks.json                  # the 5 hooks → ${CLAUDE_PLUGIN_ROOT}/bin/ctxrot
agents/ctxrot-distiller.md        # distiller subagent (keeps heavy reads off main ctx)
skills/distill/SKILL.md           # /distill skill (delegates to the subagent)
bin/ctxrot                        # POSIX launcher → ctxrot-<os>-<arch>
bin/ctxrot-<os>-<arch>            # prebuilt binaries
src/ … Cargo.toml                 # the Rust crate (reused unchanged)
```

## Configuration

`~/.ctxrot/config.toml` (created by `ctxrot init`):

```toml
store_dir = "~/.ctxrot/store"   # can point at an Obsidian vault
state_dir = "~/.ctxrot/state"
context_window = 200000         # see the warning below
large_file_bytes = 50000        # "large reference" nudge in guard (UserPromptSubmit)
huge_tool_output_bytes = 50000  # PostToolUse nudge after a big payload lands
gate_file_bytes = 1000000       # PreToolUse hard gate (deny); 0 = off
bands = [0.50, 0.75, 0.90]
```

> **⚠️ `context_window` is the *effective cap you want to stay under* — the
> target — not your model's real window.** ctxrot is a "keep it under 200K"
> guard. At the default `200000`, the 50/75/90% bands fire at ~100K / 150K / 180K
> and the preemptive rescue kicks in at ~150K. If you "correct" this to your
> real 1M window, the bands won't fire until ~950K and the tool no longer does
> anything. This is counter-intuitive, so leave it at your target on purpose.

Env overrides (Python v1 compatibility): `GUARD_DISABLE` (any value → no-op),
`CLAUDE_CONTEXT_WINDOW`, `GUARD_LARGE_FILE_BYTES`, `GUARD_GATE_FILE_BYTES`.

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
   │  preguard: Read of a 1.8MB log ──► DENIED before load ("use a sub-agent / add a limit")
   │  guard:    "推定 ~76% — /distill で退避を"   (UserPromptSubmit, once per band)
   │            └─ band ≥ 2: preemptively writes rescue-<session>-<ts>.md  ← safe to /clear NOW
   │  toolguard:"Read が ~59KB 投入 — 次回は sub-agent 経由"
   ▼
/compact ──► rescue (PreCompact): writes rescue-<session>-<ts>.md   ← nothing lost
   │   (or /clear ──► nothing fires, but the preemptive note above already saved it)
   ▼
new session ──► restore (SessionStart): injects decisions + todos + link
                (parallel-safe: routes back to THIS session's own note by tag)
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
