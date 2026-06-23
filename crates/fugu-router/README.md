# fugu-router

**fugu-style per-model routing for Claude Code orchestration.**

[Sakana AI's fugu](https://sakana.ai/fugu-release/) hides a *trained coordinator*
that routes each request across a pool of specialized models by role
(Thinker / Worker / Verifier), then verifies and synthesizes their work. The
coordinator is learned (evolution strategy / RL).

We can't train Claude's weights. So fugu-router keeps the **shape** — a separate,
deterministic component that decides *which model runs which task* — but replaces
the *trained* judgement with **retrieval over outcomes**:

```
record outcomes ── episodes.jsonl ──▶ k-NN over similar past tasks
(model, pass?, cost)                    │
                                        ▼
                          cheapest tier that historically
                          clears the bar  →  suggested_model
```

This is the honest substitution discussed in `docs/AGENTIC-CODING-GUIDE.md`:
fugu *learns* the router; we *retrieve* a policy. Coarser (per-task, not
per-token), but it needs no API key and no embedding service.

## How it maps to fugu

| fugu | fugu-router |
|---|---|
| trained coordinator | deterministic policy over an episode store |
| agent pool / tiers | Claude tiers `haiku < sonnet < opus` |
| role assignment | per task: `worker_model` + independent `verifier_model` |
| "when to delegate / which model" | cheapest tier whose similar-task pass-rate clears the bar |
| learning (CMA-ES / RL) | retrieval + **online bandit** (Thompson sampling) over logged outcomes |

### Two kinds of learning, on purpose

- **Non-parametric (retrieval):** k-NN over recorded episodes. Instant adaptation
  (one example changes behaviour), fully interpretable, trivially correctable
  (delete a bad episode). Similarity is **semantic-ish** — suffix stemming + a
  domain concept lexicon (`semantic.rs`) bridge synonyms like login ↔ auth ↔
  session that pure lexical matching would miss.
- **Parametric/online (bandit):** Thompson sampling over a Beta(passes, fails)
  posterior per tier (`policy::decide_bandit`, toggled by `explore`). It draws a
  pass-probability for each tier and picks the cheapest that clears the bar, so
  an unproven cheaper tier gets *probed* under uncertainty and the policy updates
  from the result. This is genuine online learning of the reward — not just recall.

**Limit (stated plainly):** routing is per-task, not per-turn; there is no
hidden-state head routing and no neural weight update. The bandit learns the
*reward* (which tier pays off) online; it does not learn a deep representation.
Semantic bridging is lexicon-based, not embedding-based.

## Commands

```
fugu-router route --file decomp.json [--report route.json]   # set suggested_model per task; JSON to stdout
fugu-router record --title "..." --files a,b --class parallel \
                   --model sonnet --status verified --cost 0.12   # feed an outcome
fugu-router suggest --files src/auth/login.ts "fix login validation"  # one-off
fugu-router stats [--json]                                   # per-model pass-rate / avg cost
fugu-router init                                             # write fugu-router.toml
fugu-router prompt                                           # UserPromptSubmit hook (injects a summary)
```

### `route` — the deterministic routing step

Reads a condukt decomposition, rewrites each task's `suggested_model` from
routing memory, and prints the same JSON. Pipe it straight into condukt:

```bash
condukt validate --file decomp.json
fugu-router route --file decomp.json --report /tmp/route.json > decomp.routed.json
condukt schedule --file decomp.routed.json
```

The `--report` file carries the extra advice condukt's schema has no field for —
notably `verifier_model` (an independent, usually different tier) — keyed by task id.

### `record` — the learning signal

After condukt verifies a task, log the outcome so the next run is smarter:

```bash
fugu-router record --title "<task title>" --files "<touched_files>" \
  --class parallel --model sonnet --status verified --cost 0.09
```

`--status` other than a pass-word (`verified|pass|passed|ok|true`) counts as a
non-pass. `--cost` is optional (read it from `gauge` if you want cost-aware routing).

## Integration with condukt

condukt is the orchestration spine; fugu-router is its routing brain. The
`/condukt` skill calls `route` between `validate` and `schedule`, and `record`
after each verify. The coupling is **soft**: if the `fugu-router` binary is
absent, condukt falls back to the interpreter's own `suggested_model`, so nothing
breaks. See `crates/condukt/skills/condukt/SKILL.md`.

## Install

### Plugin
Bundles the binary + the UserPromptSubmit hook. The hook injects a one-block
routing-memory summary when your prompt looks like coding work.

### Manual
```
cargo build --release
cp target/release/fugu-router ~/.cargo/bin/
fugu-router init                       # optional config
fugu-router install --dry-run          # preview settings.json change
fugu-router install                    # merge the UserPromptSubmit hook
```
Remove with `fugu-router uninstall`. Set `FUGU_ROUTER_DISABLED=1` to no-op.

## Configuration

`~/.fugu-router/config.toml` — see `fugu-router.example.toml`. Key knobs:
`pass_threshold` (how sure before trusting a cheaper tier), `min_samples` (how
much history before leaving the cold-start prior), `sim_threshold` (how similar a
past task must be to count).

## Cold start

With an empty store, routing uses a keyword prior that mirrors the interpreter's
own rule: design/refactor/migrate/security or many touched files → `opus`;
rename/format/docs/typo → `haiku`; else `sonnet`. `gated` tasks are never
auto-routed (human approval).

## License

MIT
