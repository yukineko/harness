# budgetguard

> 🌐 **English** ・ [日本語](README.ja.md)

**Real-time cost budget gate for Claude Code**, written in Rust.

gauge observes; budgetguard *controls*. On every Stop it reads the session
transcript, computes the estimated USD cost (same pricing table as gauge), and
blocks the stop if session or daily limits are exceeded — feeding the overage
notice back to the agent so it can save its work and wrap up gracefully.

It is the cost-control sibling of the verification gates:

| Gate | When | Asks |
|---|---|---|
| `donegate` | on Stop | does it build and pass? |
| `reviewgate` | on Stop | is the code quality acceptable? |
| **`budgetguard`** | **on Stop** | **is the cost within budget?** |

No API key. The transcript is already on disk; budgetguard reads it
deterministically. Nothing leaves the machine.

## How it works

`budgetguard gate` is wired to the Claude Code **Stop** hook. On each stop it:

1. reads the transcript (JSONL) and aggregates per-model token usage;
2. estimates the USD cost using the built-in pricing table;
3. updates a local **daily ledger** (`~/.budgetguard/state/ledger.json`) with
   this session's latest cost;
4. checks the session total and the day's cumulative total against configured
   limits;
5. **within limits** → exits 0, the stop proceeds;
   **warn threshold** → emits `{"additionalContext":"…"}` (advisory only, no block);
   **block threshold** → emits `{"decision":"block","reason":"…"}` asking the
   agent to save and commit before the turn ends.

### Safe by default

- No `[[session]]` or `[[daily]]` limits configured → every stop is allowed.
- A harness error (bad config, unreadable transcript, our own bug) → exits 0.
- `BUDGETGUARD_DISABLE=1` → hook is a no-op.

## Why

Left running, an LLM agent's cost climbs quietly. An observation tool like
gauge tells you *how much you spent* after the fact — but that is a post-hoc
report; it does not stop the turn that is running. A runaway loop or an
unexpectedly expensive session keeps growing until someone looks at a dashboard.

budgetguard fills that gap. By turning cost into a Stop gate, it enforces
**hard** per-session and per-day limits and hands the overage back to the agent
itself so it can land safely. It is the harness for when you need control, not
just observation.

## Install (plugin)

Via the marketplace (the catalog lives at the root of this repo, `yukineko/claude-harnesses`):

```
/plugin marketplace add yukineko/claude-harnesses
/plugin install budgetguard@yukineko
```

**Subscription-native** — it runs on a single Stop hook plus the bundled Rust
binary; no `ANTHROPIC_API_KEY`, no extra install.

## Manual install (from source)

```sh
cargo install --path .
budgetguard init          # write a starter ./budgetguard.toml
budgetguard install       # merge the Stop hook into ~/.claude/settings.json
```

## Configuration

Drop a `budgetguard.toml` in the project root, or `~/.budgetguard/config.toml`
for a global default. See `budgetguard.example.toml` for all options.

```toml
[session]
warn_usd  = 0.50
block_usd = 2.00

[daily]
warn_usd  = 5.00
block_usd = 20.00
```

## Commands

```sh
budgetguard gate           # Stop hook (reads stdin JSON, emits decision)
budgetguard status         # resolved config + today's spend
budgetguard status --json  # machine-readable budget pressure (feeds fugu-router)
budgetguard init           # write a starter budgetguard.toml
budgetguard install        # merge the hook into ~/.claude/settings.json
budgetguard uninstall      # remove it
```

## Pricing

Uses `harness-core`'s built-in rate table (same as gauge):

| Family | Input $/1M | Output $/1M |
|---|---|---|
| Fable / Mythos | 10 | 50 |
| Opus | 5 | 25 |
| Sonnet | 3 | 15 |
| Haiku | 1 | 5 |

Override any model with a `[[price]]` stanza in `budgetguard.toml`.

## License

MIT
