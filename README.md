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
| `ctxrot restore` | `SessionStart` | At session start, injects a **compact carryover** (decisions + open todos + a link). It prefers *this* session's own note (matched by session tag); the cross-session fallback returns the latest note when the stream is unambiguous (≤1 session in the dir) but, when **parallel sessions** share one project dir (≥2 sessions), restricts to untagged/shared notes so it never grabs a sibling's carryover. Never the whole note. |
| `ctxrot preguard` | `PreToolUse` | **Preventive gate, before the load.** An *unbounded* `Read` (no `limit`) of a local file at/above `gate_file_bytes` (default **1MB**) is **denied** with an actionable reason — route it through a sub-agent or re-`Read` a bounded slice. Narrow by design (only `Read`, only huge files, `limit` always bypasses) so normal source reads are untouched. Set `gate_file_bytes = 0` to disable. |
| `ctxrot toolguard` | `PostToolUse` | When a `Read`/`Bash`/`Grep`/… returns a huge payload, nudges you to route the *next* heavy read through a sub-agent and keep only conclusions. (Handles the 50KB–1MB middle band the `preguard` gate lets through.) |
| `ctxrot statusline` | `statusLine` | Always-on context-usage meter (`ctxrot 52% ▮▮▯▯ band1 ~104k/200k`), colored by band (green→yellow→red). Reads Claude's `context_window.used_percentage` from the status JSON (falls back to estimating from the transcript). `ctxrot install` sets it only when no status line exists yet, so a custom one is never clobbered. |

Plus the **`/distill` skill** for on-demand, high-quality LLM distillation (the
hooks are the cheap deterministic safety net; `/distill` is the smart one).
Distilled notes are held to a **contract**: `ctxrot note write --require-sections`
rejects (exit 1, writes nothing) any note missing the headings `restore` reads
(`決定事項/Decisions`, `残課題/Open todos`), so a schema drift fails loudly at
write time instead of silently yielding an empty carryover.

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

The Linux binary is committed directly; the **macOS binaries**
(`bin/ctxrot-darwin-arm64` / `-x86_64`) are built by the `macOS binaries` GitHub
Actions workflow on a macOS runner and committed back on each push to `main`
that touches `src/` (they can't be cross-built from Linux — Apple frameworks need
the macOS SDK). To build them by hand, run `scripts/build-plugin-bin.sh` on a Mac.

It runs entirely on your Claude subscription — the hooks and subagent execute in
the normal session model, no `ANTHROPIC_API_KEY` and no separate `cargo install`.

> **Per-user step:** each user must `/plugin marketplace add <git-url>` once
> (Claude Code does not auto-register marketplaces from a checked-in repo).
> Committing `.claude/settings.json` with `enabledPlugins` can pin *enabling*, but
> not the marketplace registration.

> **Status line is not auto-registered by the plugin.** The hooks, `/distill`
> skill and subagent load automatically, but Claude Code plugin manifests can't
> declare a general `statusLine` (`plugin.json` has no such field, and a plugin's
> `settings.json` only supports `agent`/`subagentStatusLine`). So the
> context-usage meter (`ctxrot statusline`) is **opt-in for plugin installs** —
> add it to your `~/.claude/settings.json` once:
>
> ```json
> "statusLine": {
>   "type": "command",
>   "command": "<CLAUDE_PLUGIN_ROOT>/bin/ctxrot statusline",
>   "padding": 0
> }
> ```
>
> Replace `<CLAUDE_PLUGIN_ROOT>` with the installed plugin's absolute path
> (`${CLAUDE_PLUGIN_ROOT}` expansion is only guaranteed inside `hooks.json`, not
> in `settings.json`). The manual-install path below does this for you via
> `ctxrot install`.

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
gate_bash = false               # opt-in Bash gate: deny obvious unbounded dumps
bands = [0.50, 0.75, 0.90]

reanchor_enabled = true         # re-surface decisions/todos near window end (anti lost-in-the-middle)
reanchor_min_band = 2           # only at/above this band (≈75%)
reanchor_every_prompts = 8      # at most once per N qualifying prompts

keep_notes_per_project = 30     # `ctxrot note prune` keeps the newest N
keep_distill_min = 10           # …but always protects the newest N distill notes
rescue_coalesce_secs = 120      # skip a preemptive rescue within N s of the last (0 = off)
guard_inject_max_chars = 1200   # cap the guard's own per-turn injection; over it, drop
                                #   lowest-priority first (anchor → advice → safety). 0 = off
```

> **⚠️ `context_window` is the *effective cap you want to stay under* — the
> target — not your model's real window.** ctxrot is a "keep it under 200K"
> guard. At the default `200000`, the 50/75/90% bands fire at ~100K / 150K / 180K
> and the preemptive rescue kicks in at ~150K. If you "correct" this to your
> real 1M window, the bands won't fire until ~950K and the tool no longer does
> anything. This is counter-intuitive, so leave it at your target on purpose.

Env overrides (Python v1 compatibility): `GUARD_DISABLE` (any value → no-op),
`CLAUDE_CONTEXT_WINDOW`, `GUARD_LARGE_FILE_BYTES`, `GUARD_GATE_FILE_BYTES`,
`GUARD_GATE_BASH`, `GUARD_METRICS`.

> **CJK / token-estimate note.** The byte-based thresholds (`large_file_bytes`,
> `huge_tool_output_bytes`, `gate_file_bytes`) and the `bytes/4` token estimate
> are calibrated for English prose. For Japanese and other CJK text a token is
> typically far fewer than 4 bytes (UTF-8 CJK is ~3 bytes/char, often ~1 token/
> char), so the byte→token figures shown in nudges run low. This is cosmetic: the
> **primary path is the real `usage` block** from the transcript, which is exact
> regardless of language, and the bands fire on that. The `bytes/4` proxy is only
> a fallback used when no `usage` block has been written yet. (All truncation is
> char-based via `truncate_chars`, so CJK is never cut mid-character.)

## Store

Notes are Obsidian-compatible markdown, grouped per project (keyed by cwd):
`<store_dir>/<project-basename>-<hash>/`. Inspect with:

```sh
ctxrot note list      # newest first
ctxrot note latest    # path of the most recent note
ctxrot note dir       # the project's note directory
```

## Metrics

Every hook appends one JSONL line to `<state_dir>/metrics.jsonl` — the token
**trajectory** (`budget` per prompt), every **band crossing**, rescue **note
sizes**, **gate** denies (bytes kept out of context), and tool **dumps** that
got through. Local only; disable with `metrics = false` or `GUARD_METRICS=0`.

```sh
ctxrot metrics            # per-session rollup (prompts / crossings / peak tokens / rescue / gate / dump)
ctxrot metrics path       # the metrics.jsonl path (pipe to jq for ad-hoc analysis)
ctxrot metrics compare A B # A/B two session-id prefixes; prints both groups + Δ(A−B)
ctxrot metrics peak ID    # peak % + max band for a session-id prefix (for /record to stamp in a note)
```

`ctxrot usage` prints the **current** session's usage (`ctxrot 52% … band1` + an
action `hint:`) by resolving the live transcript from `$CLAUDE_CODE_SESSION_ID`.
The `/distill` skill calls it first to act on the reading: skip when usage is low
(band 0), distill normally (band 1), or distill **and** mandate `/compact` when
high (band ≥ 2).

This is the substrate for measuring whether the guard actually holds N down.

### A/B: does the guard lower occupancy?

Run a representative heavy task twice — once guard-ON, once `GUARD_DISABLE=1` —
then fold each into a group by session-id prefix and diff:

```sh
# Group A (guard ON):  run the task in sessions whose id you can prefix, e.g. on-…
# Group B (guard OFF): GUARD_DISABLE=1, in sessions prefixed off-…
ctxrot metrics compare on- off-
```

`compare` prints both groups, the signed Δ(A−B), and a **dwell** line — the
prompts spent in each band (`b0 b1 b2 b3`). Occupancy quality is the shape, not
just the peak: a guard that works spends fewer prompts in the high bands. A
negative `peak_tok`/`band` Δ and lighter high-band dwell for A mean the guard
lowered the context high-water mark.

### Recall eval: is re-anchor worth its tokens?

Re-anchor re-injects known decisions at the window tail — *added tokens*, the
very thing ctxrot fights. It only pays off if the recall gain beats the cost, so
measure it directly. The hook never calls an LLM; the eval runs out-of-process:

```sh
cargo build --release
eval/run-recall.sh           # needs the `claude` CLI + `jq`; drives claude -p per case
# or by hand:
ctxrot eval gen --out cases --cases 9      # plants un-guessable decisions buried in filler
#   …feed each cases/*.on.txt (decision re-surfaced) and *.off.txt (not) to a model…
ctxrot eval score --manifest cases/manifest.json --results cases/results.jsonl
```

`score` prints accuracy for the OFF vs ON variants and the re-anchor added-token
cost (Σ anchor bytes /4), so the net benefit is one table:

```
variant     cases  correct  accuracy
off             9        4       44%
on              9        8       89%
re-anchor 追加注入: ~1.3k bytes (~330 tok) over 9 ON case(s)
Δ accuracy (on − off): +45 pts
```

A large positive Δ for a small token cost justifies re-anchor; a Δ near zero (or
negative) is the signal to raise `reanchor_min_band` or set `reanchor_enabled =
false`. The defaults (`reanchor_min_band = 2 ≈ 75%`, `reanchor_every_prompts =
8`) are deliberately conservative — re-anchor only fires deep into the window
(where lost-in-the-middle bites) and at most once per 8 qualifying prompts, so
its token cost stays bounded; tune them from your own eval Δ rather than priors.

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
