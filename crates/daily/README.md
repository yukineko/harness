# daily

> **Daily-once task runner for Claude Code**, written in Rust.
> A `SessionStart` hook that runs a task **at most once per calendar day** and feeds any
> findings back into the session — currently a dependency **security audit**
> (`cargo deny check`).

Some checks are worth running regularly but pointless to run on every single session
start. `daily` is the once-per-day gate for exactly those: the first session of each
calendar day pays the cost, every later session that day skips silently.

It is **subscription-native**: no API key, nothing leaves the machine. The hook is a
deterministic Rust binary; it never calls an LLM. Findings are injected as
`additionalContext` for the agent to act on — **non-blocking** (it never breaks a turn).

## What it runs

The only task today is **`security`**:

```sh
cargo deny check advisories bans sources licenses
```

| cargo-deny result | What `daily` does |
|---|---|
| success (clean) | stays silent |
| failure (findings) | injects `🔒 daily security audit: …` with the first few `error`/`warning`/`RUSTSEC` lines + a hint to run `cargo deny check` |
| cargo-deny not installed / invocation error | stays silent (never breaks the turn) |

`cargo-deny` is resolved from `$CARGO_HOME/bin/cargo-deny`, falling back to `PATH`. The
audit runs in the session's `cwd`; a repo with no `deny.toml` is fine — cargo-deny uses
its own defaults.

## The once-per-day gate

The deterministic "ran today?" logic lives in the shared
`harness-core::daily::DailyGuard`:

- State file: `~/.daily/state/<task>-daily.txt` holds the last run's `YYYY-MM-DD`.
- `should_run()` is true only when the stored date ≠ today; `mark_done()` stamps today.
- The gate keys on **calendar day** (local time), not wall-clock hours — so exactly one
  run per day regardless of how many sessions open.

## The hook

| Hook | Event | What it does |
|---|---|---|
| **`daily session-start`** | `SessionStart` (startup/resume/clear) | if enabled and not yet run today, runs the security audit, stamps `mark_done()`, and injects findings (if any). Always exits 0. |

## Configuration

`~/.daily/config.toml` (optional):

```toml
enabled = false   # disable all daily tasks
```

A missing config means **enabled**. The current check is a simple `enabled = false`
substring match — set it to turn the runner off entirely.

## Subcommand surface

| Subcommand | Purpose |
|---|---|
| `daily session-start` | SessionStart hook: run pending daily tasks |
| `daily install` | (not yet implemented) — add the hook to `~/.claude/settings.json` manually |

## Install

### As a Claude Code plugin (recommended)

```text
# in Claude Code:
/plugin marketplace add yukineko/harness
/plugin install daily@yukineko
```

The hook calls `${CLAUDE_PLUGIN_ROOT}/bin/daily session-start`. `bin/daily` is a POSIX
launcher that selects the right per-platform binary (`bin/daily-<os>-<arch>`); if a host
has no matching binary it exits 0 silently. `cargo-deny` must be installed separately
(`cargo install cargo-deny`) for the security task to do anything.

### Build from source

```sh
scripts/build-plugin-bin.sh
git add bin/ && git update-index --chmod=+x bin/daily bin/daily-*
```

## Platform support

| Host | File | Status |
|---|---|---|
| macOS Apple Silicon | `bin/daily-darwin-arm64` | bundled |
| Linux x86_64 | `bin/daily-linux-x86_64` | build with `scripts/build-plugin-bin.sh` on Linux |
| macOS Intel | `bin/daily-darwin-x86_64` | built in CI on a macOS runner |

## Plugin layout

```
.claude-plugin/plugin.json     # plugin manifest (version 0.1.0)
hooks/hooks.json               # SessionStart=session-start → ${CLAUDE_PLUGIN_ROOT}/bin/daily
bin/daily                      # POSIX launcher → daily-<os>-<arch>
bin/daily-<os>-<arch>          # prebuilt binaries
src/main.rs … Cargo.toml       # the Rust crate (uses harness-core::daily::DailyGuard)
```

## Development

```sh
cargo test -p daily
cargo build -p daily
```

## License

MIT
