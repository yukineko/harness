# specguard

> 🌐 **English** ・ [日本語](README.ja.md)

**Spec ↔ implementation drift audit harness (project-agnostic)**

A CLI that has an LLM agent audit, **read-only**, whether an implementation has
drifted from its *canonical spec*, and whether the canon docs themselves contain
silence, contradiction, or duplication. The judgment lives in the LLM (which
quotes the canon verbatim); specguard is the **deterministic harness** around it
— scope resolution → prompt rendering → agent launch → marker parsing →
report / sentinel. Everything project-specific is externalized to a TOML config.

```
specguard.toml ──┐
   git diff ──────┼──▶ scope (changed areas ∪ invariants)
                  │         │
templates/ ───────┼──▶ render prompt ──▶ agent (read-only) ──▶ parse markers ──▶ report + sentinel
```

There are two ways to run it, both sharing **the same `specguard` binary**:

| | standalone binary | Claude Code plugin |
|---|---|---|
| Audit engine | spawns `claude --print` per shard | in-session read-only subagent (no nested claude) |
| Billing | depends on the claude CLI login | **the host session's subscription** |
| read-only enforcement | `claude --print` **Bash-arg allowlist** (strong) | subagent **tool-name** restriction (weaker) |
| Entry point | `specguard run` (cron, etc.) | `/specguard:run` (interactive / HOTL) |

→ For the design and invariants see **[DESIGN.md](DESIGN.md)** /
**[DESIGN-VERIFY.md](DESIGN-VERIFY.md)** (Japanese); the canon for the audit
policy (classification, verdict vocabulary, discipline) is
`templates/audit-prompt.md`.

---

## Getting started

### Prerequisites

- A Rust toolchain (`cargo`). Get it from https://rustup.rs.
- The audit target must be a **git repository**.
- Either way the target repo needs a `specguard.toml` (scaffolded below).

### 1. Install the binary (prerequisite for both modes)

```sh
./install.sh                                  # release build → ~/.local/bin
SPECGUARD_BIN_DIR=/usr/local/bin ./install.sh # to change the install dir
```

Manually, `cargo build --release` produces `target/release/specguard`. Make sure
`~/.local/bin` is on your PATH. Details and troubleshooting are in
**[INSTALL.md](INSTALL.md)**.

### 2. Scaffold into the target repo

```sh
cd /path/to/your/repo
specguard init        # generates specguard.toml + a SessionStart hook (idempotent)
```

`init` will not overwrite an existing `specguard.toml` without `--force`, and it
appends only the SessionStart hook (which surfaces unhandled drift) without
disturbing other settings in `.claude/settings.json`. **In plugin mode the hook
is bundled**, so you only need the config (`cp specguard.example.toml specguard.toml`
also works).

### 3a. Use it standalone

```sh
cd /path/to/your/repo
# edit specguard.toml's [[area]] / [[invariant]] / canon for your repo
specguard run                                 # run the audit
```

Run `specguard run` from cron / a task scheduler, and let the SessionStart hook
pick up the sentinel raised when `needs_user=yes`, prompting a human — a
Human-on-the-loop loop.

### 3b. Use it as a Claude Code plugin (subscription-native)

This repository *is* the plugin. Instead of spawning `claude --print`, it
delegates each shard to an in-session read-only subagent (`specguard-auditor`),
auditing on the host session's subscription. The deterministic harness is still
delegated to the same `specguard` binary (no duplicated judgment logic).

```sh
cd /path/to/your/repo
claude --plugin-dir /path/to/specguard        # load it for this session
# after edits: /reload-plugins; inspect with /plugin
```

```
/specguard:run
  └─ specguard prompt --json    (harness: resolve scope + render shards)
  └─ Task(specguard-auditor) × shard   (judgment: read-only subagent / subscription)
  └─ specguard ingest --from …  (harness: parse → verify → report → sentinel/baseline)
```

---

## Usage

Once a human handles a `needs_user=yes` finding, clear the sentinel (otherwise
the SessionStart hook keeps nagging about the same issue).

### Slash commands (plugin)

| Command | Backing binary | Description |
|---|---|---|
| `/specguard:run [--baseline <ref>]` | `prompt --json` + subagent + `ingest` | subscription-native audit |
| `/specguard:scope` | `scope` | show the resolved scope (no agent) |
| `/specguard:ack` | `ack` | clear a handled sentinel |
| `/specguard:accept-prompt <reason>` | `accept-prompt` | ratify & pin the prompt (meta-canon) |
| `/specguard:decide <title>` | `decide` | scaffold a decision record (ADR) pinned to the canon commit |

### Subcommands (binary)

```sh
specguard run                      # run the audit (spawns claude --print per shard)
specguard scope                    # print the resolved scope only (no agent)
specguard prompt                   # print each shard's prompt (no agent)
specguard prompt --json            # emit shards as machine-readable JSON (used by the plugin)
specguard ingest [--from <file>]   # feed pre-collected shard outputs (JSON/stdin) into
                                   #   parse→report→sentinel (does NOT spawn an agent)
specguard ack                      # clear a handled sentinel
specguard decide "<title>"         # scaffold a decision record (ADR)
specguard accept-prompt -m "reason"  # ratify the prompt (meta-canon)
specguard --baseline HEAD~5 run    # override the baseline
specguard --config examples/aegis.toml run
```

### Output

| Path | Contents |
|---|---|
| `<report_dir>/<date>.md` | the report |
| `<report_dir>/.last-ref` | the last audited HEAD (next run's change-triggered baseline) |
| `<sentinel>` | only when `needs_user=yes` (date / report / summary) |

The baseline **advances in lockstep with ack**: `.last-ref` moves to HEAD only on
a clean run, and is held while findings remain (so unfixed drift can't fall out
of the next run's diff and go undetected).

---

## Configuration (TOML)

`specguard.example.toml` has a fully commented sample of every field. Key points:

- `[project]` … `name`, `root` (repository root)
- `[agent]` … `command` + `args`. Defaults to `claude --print` (with a read-only
  allowlist). Swappable for any agent CLI (reads a prompt on stdin, writes the
  report to stdout)
- `[scope]` … `baseline_ref` / `fallback_ref` (if neither resolves, all tracked
  files are audited)
- `[output]` … `report_dir` / `sentinel`
- `[prompt]` … `template` (embedded default if omitted) / `require_ratification`
  (the ratification gate)
- `[[area]]` (repeatable) … `name` / `globs` / `canon`. **In-scope when a change
  matches `globs`**
- `[[invariant]]` (repeatable) … `name` / `description` / `canon`. **Checked
  every run**
- `[verify]` … verification gates (default OFF). `enabled` = refutation (drop
  false positives) / `completeness` = completeness critique (surface false
  negatives). **Enabling both is recommended.** See [DESIGN-VERIFY.md](DESIGN-VERIFY.md)
- `[decisions]` … enable the decision-record (ADR) freshness/staleness check (D3)

`examples/aegis.toml` is a config reproducing the original AEGIS implementation.

### The three audit dimensions (overview)

The canon is the audit prompt (`templates/audit-prompt.md` /
`decisions-prompt.md`). In brief:

- **D1 implementation↔canon drift**: has the implementation drifted from the
  canon (contradictions classified as misread / code-violation / stale-canon).
- **D2 spec quality**: silence / contradiction / duplication in the canon docs
  themselves.
- **D3 decision-log freshness/staleness**: pin the *reason* for a spec change to
  a canon commit and check whether the decision still holds (enable via
  `[decisions]`).

---

## Exit codes

| code | meaning |
|---|---|
| 0 | success |
| 2 | config / usage error |
| 3 | a shard's output lacked the marker (report saved; baseline not advanced, no sentinel) |
| 4 | a shard's agent exited non-zero (the real code goes to stderr) |
| 5 | the prompt (meta-canon) is unratified/changed (when `require_ratification` is on); needs `accept-prompt` |

The source of truth is the `EXIT_*` constants in `src/main.rs` (this is the only
doc copy of the table). Agent exit codes are never propagated raw — they always
collapse to `4`, with each shard's real code on stderr.

---

## About the read-only guarantee

- **standalone**: the default agent launches with an allowlist (Read/Grep/Glob +
  `git diff/log/show/status`) and denies writes, network, and arbitrary shell. In
  `--print` mode any tool outside the allowlist is auto-denied, so even a prompt
  injection from the audited repo's content cannot run a destructive command. It
  is guaranteed by **permissions**, not by a polite request in the prompt.
- **plugin**: the subagent guarantee is at the **tool-name** level (Edit/Write/
  NotebookEdit/WebFetch/WebSearch revoked + a read-only-git prompt discipline). A
  Claude Code subagent definition cannot express a Bash *argument* allowlist
  (`Bash(git diff *)`), so it is weaker against prompt injection than standalone.
  For targets where enforcement strength matters most, prefer standalone
  `specguard run`.
- When the verification gates (`[verify]`) are on, the refutation/completeness
  steps inside `ingest` still spawn an agent via the binary (so a nested claude
  runs even through the plugin). Full native-ization is future work.

---

## Tests

```sh
cargo test          # unit (parse/scope/prompt/report) + integration (fake agent)
```

The integration tests use a `bash -c` fake agent, so no real LLM is required.

## License

MIT
