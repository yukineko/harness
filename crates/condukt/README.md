# condukt

A **deterministic orchestration engine** for Claude Code.

Large tasks decompose into many small ones. The judgement — interpret the
request, implement each piece, verify it — is LLM work. But deciding *which
tasks can run in parallel*, *managing git worktrees*, *tracking run state*, and
*knowing when you are actually done* should not be eyeballed by a language model.
condukt splits the two:

```
LLM  (the /condukt skill + interpreter/worker/verifier agents)
  ├ interpret the request        ─┐
  ├ decompose into tasks (JSON)   │   condukt binary (deterministic)
  ├ implement each task           ├──▶ schedule:  conflict analysis → parallel/serial batches
  └ verify against criteria       │    worktree:  create / merge / remove / cleanup
                                  ─┘    state:     run tracking + completion gate
```

The binary is a single Rust executable exposing one subcommand per job. It is
**subscription-native**: no `ANTHROPIC_API_KEY`, no separate install for plugin
users — the work runs inside Claude Code via a skill, three agents, and one
SessionStart hook.

## What the engine does

| subcommand | purpose |
|---|---|
| `condukt schedule` | read a decomposition JSON, output ordered parallel batches + serial/gated lists. Two tasks share a batch only if their `touched_files` don't conflict and neither depends on the other. |
| `condukt validate` | check a decomposition JSON (unique ids, known deps, no cycles). |
| `condukt worktree create/merge/remove/cleanup/list` | git-worktree lifecycle; enforces "path outside the repo" and "one dir = one branch". |
| `condukt state init/set/show/gate/list` | persist a run's task statuses; `gate` exits non-zero until every task is verified and no worktree is left dirty or unremoved. |
| `condukt state conflict-check/abandon/list-tasks/cancel/pause` | cross-session safety + run editing: detect file/goal conflicts before `init`, return stuck `running` tasks to `pending` (`--all-stuck`), list/cancel a run's tasks, pause a conflicting run (see the skill's Phase 0/3.5 and the cancel utility). |
| `condukt knowledge` | emit project-specific conventions/pitfalls injected into the interpreter/worker prompt (soft; empty when none). |
| `condukt consensus plan/vote` | multi-sample self-consistency (opt-in cost guard). `plan` decides whether a task should fan out into N candidate implementations (exit 0 = fan out, 1 = single sample); `vote` tallies N verifier verdicts into a deterministic majority winner + agreement rate, escalating to opus on all-fail, a tie, or agreement below threshold. |
| `condukt state stats` | aggregate all runs (complete and incomplete): completion rate, task count, status distribution — useful as a before/after benchmark. |
| `condukt state reconcile --run <id> [--dry-run]` | auto-promote tasks to `verified` when their branch is already merged into the default branch or has been deleted with its worktree. Fixes stale state after a session crash without manual `state set` calls. |
| `condukt state resume-context --run <id>` | emit pending/failed/done tasks as JSON for resuming a stopped run across sessions (see Phase 0-alt in the skill). |
| `condukt state test` | run the project's test suite from the repo root (auto-detects `cargo test` / `npm test` / `pytest`, or uses `[test].command` from config). |
| `condukt restore` | SessionStart hook: reminds you of unfinished runs / orphan worktrees. |
| `condukt statusline` | one-line run progress for the `statusLine` setting. |
| `condukt init / install / uninstall` | create `~/.condukt`; manual hook wiring (plugin users don't need these). |

The decomposition schema (what the interpreter agent emits / `schedule` consumes).
Canonical definition: `agents/condukt-interpreter.md`.

```json
{ "goal": "...", "linked_hypotheses": ["hid1"],
  "tasks": [
  { "id": "t1", "title": "...", "touched_files": ["path/or/glob"],
    "deps": ["t0"], "class": "parallel|serial|gated",
    "suggested_model": "sonnet|opus|haiku", "done_criteria": "observable pass condition" }
]}
```

## Install

### Plugin (recommended)

> The marketplace catalog lives in a separate central repo. Once condukt is
> published there, install is:

```
/plugin marketplace add <git-url-of-the-catalog-repo>
/plugin install condukt@yukineko
```

This bundles the `/condukt` skill, the three agents, the SessionStart hook, and a
prebuilt binary. Optionally run `condukt init` once to create `~/.condukt` and a
default `config.toml`.

### Manual (build from source)

```
cargo build --release
cp target/release/condukt ~/.cargo/bin/      # or anywhere on PATH
condukt init
condukt install --dry-run                    # preview settings.json changes
condukt install                              # merge the SessionStart hook (backs up settings.json)
cp -r skills/condukt ~/.claude/skills/        # and agents/ -> ~/.claude/agents/
```

Remove with `condukt uninstall`.

## Configuration

`~/.condukt/config.toml` (defaults shown):

```toml
worktree_base  = "~/.condukt/worktrees"  # MUST be outside any repo
default_branch = "main"
max_parallel   = 4                        # advisory soft cap on concurrent workers
shared_globs   = []                       # globs that force a touching task to run serially

# Command `condukt state test` runs (via `sh -c`, from the repo root).
# Omit to auto-detect (cargo test / npm test / pytest).
# [test]
# command = "cargo test"

# Multi-sample self-consistency (OPT-IN cost guard; OFF by default). When
# enabled, a high-risk task is implemented N times, verified, and a majority
# vote picks the winner; low agreement escalates to opus. N-sample generation
# is N x the cost. A per-task `consensus plan --risk high` forces fan-out even
# when enabled = false. samples is clamped to a ceiling of 5.
# [consensus]
# enabled   = false
# samples   = 3
# threshold = 0.5
```

`shared_globs` is how you keep workers off project-wide files without hardcoding
anything — e.g. `["**/models.py", "**/migrations/**", "docs/glossary.md"]`. Any
parallel task touching one is demoted to serial with a warning.

### Environment variables

All config file keys can be overridden at runtime with environment variables.
`CONDUKT_DISABLE` is a hook-only kill switch and has no config file equivalent.

| Variable | Default | Description |
|---|---|---|
| `CONDUKT_WORKTREE_BASE` | `~/.condukt/worktrees` | Directory where worktrees are created (must be outside any repo). |
| `CONDUKT_DEFAULT_BRANCH` | `main` | Branch completed work is merged back into. |
| `CONDUKT_MAX_PARALLEL` | `4` | Advisory soft cap on concurrent workers. |
| `CONDUKT_DISABLE` | _(unset)_ | Set to `1` to make the SessionStart/statusline hooks no-op (useful in CI). |
| `CONDUKT_CONSENSUS` | `false` | Set to `1`/`true` to enable multi-sample self-consistency fan-out (overrides `[consensus] enabled`). Opt-in cost guard; off by default. |

### `condukt loop` — test-fix cycle

Runs one iteration of a test-fix cycle for a given module type and prints a JSON
result. The `/condukt-loop` skill calls this repeatedly, inserting a fix step
between iterations, until all tests pass or no progress is detected.

```
condukt loop --module <server|client|e2e> [--iteration N] [--prev-failures N]
```

**Cycle sequences** (configured via `[loop]` in `config.toml`):

| `--module` | Steps |
|---|---|
| `server` | deploy → test |
| `client` | build → test |
| `e2e` | build → deploy → test |

**JSON output** (one object per invocation):

```json
{
  "iteration": 1,
  "module": "client",
  "failure_count": 3,
  "success": false,
  "stop": false,
  "stop_reason": "",
  "output": "<combined stdout+stderr>"
}
```

`stop=true` when `failure_count == 0` (`stop_reason: "all tests pass"`) or when
`failure_count == prev_failures` (`stop_reason: "no progress: failure count unchanged"`).

**Config:**

```toml
[loop]
build_command  = "npm run build"
deploy_command = "kubectl rollout restart deployment/api && kubectl rollout status deployment/api"
max_iters      = 10   # safety cap; the skill enforces it
```

### `condukt state test`

Runs the project's test suite from the repo root and propagates its exit code.

```
condukt state test --run <run-id>
```

The command source is resolved in this priority order:

1. `[test].command` in `~/.condukt/config.toml`
2. Auto-detected from the repo root: `cargo test` (Cargo.toml), `npm test` (package.json), `pytest` (pyproject.toml / setup.py), falling back to `cargo test`.

The command is executed via `sh -c`, so quoted arguments, pipes, and env-var
expansions all work as expected — e.g. `command = "pytest -k 'unit or smoke'"`.
Running from the repo root (not the cwd of the caller) means auto-detection always
sees the project manifest even when the caller is in a subdirectory.

## Constraints

- **Per-machine marketplace step.** Each user runs `/plugin marketplace add <url>`
  once — Claude Code does not auto-register a marketplace from a checked-in repo.
- **Per-platform binaries.** Linux x86_64 is committed in `bin/`. macOS arm64 /
  x86_64 are built by the GitHub Actions macOS runner (Apple SDK can't cross-build
  from Linux). If the host has no matching binary the launcher exits 0 with a build
  hint, so a hook never breaks a turn.
- **Exec bits.** Binaries and the launcher must keep their exec bit in the git
  index (`git update-index --chmod=+x bin/condukt bin/condukt-*`), because the repo
  is often checked out on a `core.filemode=false` mount.

## Development

```
cargo test          # unit tests (scheduling, gate, project key)
cargo clippy --all-targets
scripts/build-plugin-bin.sh        # stage bin/condukt-<os>-<arch> for the host
```

### Source of truth: edit the repo, not the cache

`crates/condukt/` (this directory) is the **single source of truth**. `/plugin
install` copies it to `~/.claude/plugins/cache/<owner>/condukt/<version>/` as a
plain copy (no `.git`), and the running `/condukt` skill loads its agents and
`SKILL.md` from there. Editing that cache copy — easy to do by accident when you
use condukt to improve condukt itself — produces edits that live outside git and
silently diverge from the repo.

Rule: **never hand-edit the cache.** Edit the files here, then refresh your local
install. When condukt orchestrates a change to its **own** plugin, point the
workers at this repo (a git worktree of it), never at the cache path.

```
scripts/sync-plugin-assets.sh           # repo -> cache: refresh your local install
scripts/sync-plugin-assets.sh --check   # report drift; exit 1 if cache != repo
```

Run `--check` before committing (or wire it into a pre-push hook) to catch a
cache that has drifted from the repo, or a new agent/skill file that was created
in the cache but never committed.

## License

MIT
