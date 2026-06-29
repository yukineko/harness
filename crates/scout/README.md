# scout

Multi-lens project audit that generates actionable tasks (施策) for Claude Code.
The `/scout` skill gathers deterministic project state, fans out read-only
sub-agents across five lenses — current issues, security, industry/peer-project
practices (via web search), missing measures, and safety — then dedupes, scores,
and prioritizes the findings into tasks, writes the approved ones to the
backlog, and hands execution to `/flow`. Where compass is a single-goal
gradient, scout is broad reconnaissance: it surfaces many independent measures
at once.

Subscription-native: **skill only, no binary, no API key**. scout holds no state
and never edits files — it only *discovers*. Storage is delegated to the
`backlog` binary and execution to `/flow` → `/condukt`.

## What it does

`/scout` runs a fixed pipeline, with the LLM doing the judgement and the
sub-agents doing read-only investigation:

| Phase | What happens |
|---|---|
| Scope + reduce | Take the audit scope; reduce lens count by scope size to control cost (small → L1/L5, large → all 5) |
| Deterministic review | Collect facts read-only (`git log`, `cargo test`, `compass gap`, `backlog list`, `cargo deny`, …) |
| 5-lens fan-out | Parallel read-only sub-agents return measure candidates with verbatim evidence |
| Synthesize | Dedupe, evidence-filter, score `(severity × goal-proximity) ÷ effort`, tag `p0/p1/p2` |
| Agree (HOTL) | `AskUserQuestion` (multiSelect) — the user picks which measures to queue |
| Write + hand off | `backlog add` each approved measure (`--tag scout`), then propose `/flow` |

Invariants: the audit is read-only, no measure ships without evidence (verbatim
quote / `file:line` / source URL), the only write is `backlog add`, and queuing
requires explicit user agreement.

## Install (plugin)

Installed via the plugin marketplace, the `/scout` skill is available
immediately — there is no binary and no hook to wire. It relies on the `backlog`
binary being on PATH to persist measures; without it, scout presents the
measures as Markdown for manual entry.

## Using the skill

```
/scout                      # audit the whole repo, all five lenses
/scout security のみ          # restrict to the security lens
/scout crates/condukt        # audit a subtree only
/scout --dry-run             # present measures, but don't write to backlog
```

After it queues measures, scout proposes `/flow` (propose-then-confirm) and
stands down — the execution loop and backlog lock are flow's responsibility,
not scout's.

## Build

```sh
cargo test
```

scout is skill-only, so the `cargo test` here covers the workspace; the plugin
ships no binary of its own.
