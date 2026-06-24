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
| `condukt restore` | SessionStart hook: reminds you of unfinished runs / orphan worktrees. |
| `condukt statusline` | one-line run progress for the `statusLine` setting. |
| `condukt init / install / uninstall` | create `~/.condukt`; manual hook wiring (plugin users don't need these). |

The decomposition schema (what the interpreter agent emits / `schedule` consumes):

```json
{ "goal": "...", "tasks": [
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

`~/.condukt/config.toml` (defaults shown; env overrides in parentheses):

```toml
worktree_base  = "~/.condukt/worktrees"  # MUST be outside any repo  (CONDUKT_WORKTREE_BASE)
default_branch = "main"                   #                          (CONDUKT_DEFAULT_BRANCH)
max_parallel   = 4                        # advisory soft cap        (CONDUKT_MAX_PARALLEL)
shared_globs   = []                       # globs that force a touching task to run serially
```

`shared_globs` is how you keep workers off project-wide files without hardcoding
anything — e.g. `["**/models.py", "**/migrations/**", "docs/glossary.md"]`. Any
parallel task touching one is demoted to serial with a warning.

Set `CONDUKT_DISABLE=1` to make the hooks no-op.

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
