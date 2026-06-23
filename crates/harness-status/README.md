# harness-status

**Unified HOTL status dashboard for Claude Code**, written in Rust.

The harness plugins each keep their own state. harness-status reads them all and
prints one human-on-the-loop view — what you'd glance at to decide whether to
step in:

- **Budget** (budgetguard): today's spend and session count, from
  `~/.budgetguard/state/ledger.json`.
- **Recent sessions** (gauge): the last N sessions with turns, tokens, and USD
  cost, from `~/.gauge/store/sessions/`.
- **Progress** (taskprog): the current `.claude/progress.md` preview.

It is **read-only** — it never writes, only aggregates other plugins' stores. No
hooks, no API key: a single binary you (or a `/status` command) run on demand.

## Output

```
╔══════════════════════════════════════════════╗
║         harness-status  (2026-06-23)         ║
╚══════════════════════════════════════════════╝

── Budget (budgetguard) ──────────────────────────
  Today spend:  $1.8420  (3 session(s))

── Recent sessions (gauge) ───────────────────────
  Session          Project              Turns       Tokens  Cost USD
  ----------------------------------------------------------------------
  3c8d91a2         harness                 12        35000    0.1850

── Progress file (taskprog) ──────────────────────
  cwd: /repo
  /repo/.claude/progress.md
  │ # Progress
  │ ## Pending
  │ - specforge ⑤ worktree merge
```

## Install (plugin)

```
/plugin install harness-status@yukineko
```

Then run `/status` in any session.

## Manual install

```sh
cargo install --path .
harness-status            # full dashboard
```

## Commands

```sh
harness-status                       # full dashboard
harness-status budget                # today's spend only
harness-status sessions --sessions 10  # recent sessions (limit N)
harness-status progress              # progress file only
harness-status --json                # machine-readable output (any subcommand)
```

## Notes

- A section that reports "not installed" just means that plugin's store is
  absent — not an error.
- The date is derived without a clock dependency; override with `HARNESS_DATE=YYYY-MM-DD`
  for testing.

## License

MIT
