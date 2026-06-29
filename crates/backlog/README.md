# backlog

Cross-project task queue for Claude Code — a durable queue of work items, tagged
by cycle type, that outlives any one session and any one repo. backlog surfaces
pending work the moment a session opens (a **SessionStart** hook injects it as
context) and exposes a small binary for adding, picking, and resolving items.
The lock→pick→`/condukt`→done driver loop lives in `/flow`; the bundled
`/backlog` skill is a thin alias to it.

Subscription-native: a skill, one hook, and a bundled Rust binary, **no API
key**. The SessionStart hook is fail-soft — malformed stdin is logged to stderr
and skipped, and the hook always exits 0 so a turn is never broken.

## What it does

The `backlog` binary owns the queue and its exclusive run-lock:

| Subcommand | What it does |
|---|---|
| `add` | Append a task (`--title`, `--project`, `--tag`, `--priority p0/p1/p2`, `--notes`, `--weight`) |
| `list` | List tasks, filterable by `--tag` / `--project` / `--status` |
| `next` | Print the next highest-priority pending task as JSON |
| `done <id>` | Mark a task done |
| `fail <id>` | Mark a task failed (`--reason`); defers re-run by 2 days |
| `edit <id>` | Update a task's title / tags / notes / status |
| `session-start` | SessionStart hook: inject pending tasks as context |
| `install` / `uninstall` | Wire/remove the SessionStart hook in `~/.claude/settings.json` |
| `lock {acquire,release,status}` | Manage the `~/.backlog/run.lock` exclusive lock |

The lock is how concurrent sessions serialize: a `/flow` driver acquires it
before draining the queue, and other sessions back off when `lock status`
reports an active holder.

## Install (plugin)

Installed via the plugin marketplace, the bundled `/backlog` skill is available
immediately. The SessionStart hook is registered by running `backlog install`,
which merges a `SessionStart` group into `~/.claude/settings.json` (idempotent,
ownership-marked) so pending work shows up at every session open.

## Standalone (cargo)

```sh
cargo install --path .
backlog add --title "Fix X" --project "$PWD" --priority p1   # queue an item
backlog list --status pending                                # see the queue
backlog next                                                 # pick the next item
backlog done <id>            # resolve it
backlog fail <id> --reason "blocked"   # defer it 2 days
backlog lock status         # who holds the run-lock
backlog install             # merge the SessionStart hook into settings.json
backlog uninstall           # remove it again
```

`install`/`uninstall` accept `--dry-run` to print the resulting settings without
writing.

## Build

```sh
cargo test
```

The committed `bin/backlog-*` binaries are what the plugin ships, so end users
need neither cargo nor an API key. Rebuild and recommit them (the workspace
builds with `cargo build --workspace --release`) when you change behavior the
skill or hook relies on.
