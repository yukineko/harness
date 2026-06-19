# donegate

> **Completion-verification gate for Claude Code**, written in Rust.

Agents declare victory too early. `donegate` is the harness that makes "done"
mean *verified*: on every **Stop**, it runs your acceptance commands
(`build` / `test` / `lint` / `typecheck`) as **subprocesses**, and refuses to let
the turn end until every required check is green. A failing check's output is fed
straight back into the conversation, so the agent enters an **auto-fix loop**
instead of stopping on red.

It is the *dynamic* sibling of two static gates:

| Gate | When | Asks |
|---|---|---|
| `precommit-audit` | pre-commit | does the diff obey policy? (static) |
| `specguard` | on demand | did the impl drift from spec? (LLM) |
| **`donegate`** | **on Stop** | **does it actually build & pass? (runs it)** |

No API key. The LLM labor (fixing the failures) runs in your Claude Code
subscription; donegate is a deterministic Rust binary that only spawns processes
and reads exit codes — so it can never hit a rate limit.

## How it works

`donegate gate` is wired to the Claude Code **Stop** hook. On each stop it:

1. reads the hook JSON from stdin (`session_id`, `cwd`, `stop_hook_active`);
2. loads `./donegate.toml` (or `~/.donegate/config.toml`);
3. asks `git` which files changed, and selects the checks whose `when_changed`
   globs match (unscoped checks always run);
4. runs each selected check as a subprocess, with a per-check timeout, capturing
   a **bounded tail** of its output (never the whole log);
5. **all green** → exits 0, the stop proceeds;
   **any required failure** → emits `{"decision":"block","reason":…}` so Claude
   keeps working, with the failing command + output tail in the reason.

### Loop safety

- A per-session **attempt counter** gives up after `max_attempts` consecutive
  blocks (default 3) and allows the stop, so a genuinely stuck agent isn't
  trapped. The counter resets after `reset_after_secs` of idle, or on any green.
- **Escape hatch**: create `.donegate-skip` (one-line reason) in the project
  root — consumed once, allows the next stop.
- **Kill switch**: `DONEGATE_DISABLE=1`.
- **Safe by default**: with no `[[check]]` configured, the gate allows every
  stop. Installing the hook can never block a project that hasn't opted in.

A *harness* error (bad config, our own bug) always exits 0 and lets the stop
through. Only an actual failing check blocks — on purpose.

## Install

```sh
cargo install --path .
cd your/project
donegate init          # writes a starter donegate.toml for your stack
donegate install       # merges the Stop hook into ~/.claude/settings.json (backs it up)
```

Then check what would run:

```sh
donegate status        # resolved config + which checks apply to the current diff
donegate gate          # run the checks by hand (human report; exit 1 on failure)
```

Remove with `donegate uninstall`.

## Config

See [`donegate.example.toml`](donegate.example.toml). Each `[[check]]`:

| key | meaning |
|---|---|
| `name` | label in the block reason |
| `cmd` | shell command (`sh -c` / `cmd /C`) |
| `when_changed` | globs vs `git diff HEAD` + untracked; omit ⇒ always run |
| `timeout_secs` | per-check timeout (default `default_timeout_secs`) |
| `optional` | `true` = warn on failure, don't block |
| `workdir` | run in this subdir of the project root |

`donegate init` auto-detects Cargo / npm / Python / Go and writes matching
checks.

## Logs

Every decision appends one JSONL line to `~/.donegate/state/log.jsonl`
(`verdict` ∈ `green` / `blocked` / `giveup` / `skip`). Local only.

## License

MIT
