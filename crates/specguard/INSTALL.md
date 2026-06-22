# specguard install guide

> 🌐 **English** ・ [日本語](INSTALL.ja.md)

Everything needed to bring up the spec↔implementation drift audit harness
specguard — from building, to wiring it into a target repo, to scheduled runs.
For the overview and design see [README.md](README.md).

---

## 1. Prerequisites

| Requirement | Used for | Notes |
|---|---|---|
| **Rust toolchain (`cargo`)** | building specguard | install from https://rustup.rs |
| **`git`** | resolving the change scope (`git diff` / `ls-tree`) | the audit target must be a git repo |
| **`claude` CLI** (Claude Code) | the audit agent (default) | only needed for `specguard run`; must be authenticated |

`init` / `scope` / `prompt` / `ack` work without the `claude` CLI (they launch no
agent). Only `run` — the actual audit — needs `claude`. You can also swap in a
different agent (see `[agent]` below).

---

## 2. Install the binary

### Using install.sh (recommended; WSL2 / Linux / macOS)

```sh
./install.sh
```

- Builds with `cargo build --release` and places it at `~/.local/bin/specguard`.
- To change the destination, use the env var:

  ```sh
  SPECGUARD_BIN_DIR=/usr/local/bin ./install.sh
  ```

- If `~/.local/bin` isn't on your PATH, the script tells you how to add it, e.g.:

  ```sh
  echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc && source ~/.bashrc
  ```

Verify:

```sh
specguard --version
specguard --help
```

### Manual build

```sh
cargo build --release
# artifact: target/release/specguard
cp target/release/specguard ~/.local/bin/    # to any dir on PATH
```

### Windows (native)

`install.sh` is for bash. On native Windows, build and place it with cargo:

```powershell
cargo build --release
# copy target\release\specguard.exe to a directory on PATH
```

---

## 3. Wire it into the target repo

Run `init` at the root of the repo you want to audit:

```sh
cd /path/to/your/repo
specguard init
```

Artifacts:

| Path | Contents |
|---|---|
| `specguard.toml` | starter config (a copy of `specguard.example.toml`) |
| `.claude/settings.json` | SessionStart hook (runs `specguard pending`, which surfaces an active fix-offer when a sentinel is raised) |

`init` is **idempotent**:

- it won't overwrite an existing `specguard.toml` without `--force`.
- it preserves other settings in `.claude/settings.json` and only appends the
  SessionStart hook; re-running never duplicates the hook.

To recreate the config:

```sh
specguard init --force      # overwrite specguard.toml with the example
```

> **Plugin mode**: when you use the Claude Code plugin, the SessionStart hook is
> bundled, so you only need a `specguard.toml` (no hook setup). See the plugin
> section in [README.md](README.md).

---

## 4. Edit the config (`specguard.toml`)

The config right after `init` is a sample; edit it for your repo. Key points:

```toml
[project]
name = "MyProject"
root = "."                  # repo root to audit (relative to this config file)

# Omitting [agent] uses the default (claude --print) with harness-enforced
# read-only. Only set it to swap in a different agent.

[scope]
baseline_ref  = ""          # empty => resolve via .last-ref then fallback_ref
fallback_ref  = "HEAD~20"   # baseline for the first run / when unresolvable

[output]
report_dir = "reports/spec-audit"
sentinel   = ".specguard-pending"

# area: in-scope when a change matches its globs. canon points at "what to read".
[[area]]
name  = "backend"
globs = ["src/server/**", "api/**"]
canon = ["docs/architecture/api.md"]

# invariant: an absolute rule checked every run regardless of changes.
[[invariant]]
name        = "secrets path"
description = "secrets are only read from the approved config path"
canon       = ["docs/architecture/config.md"]
```

- At least one `[[area]]` or `[[invariant]]` is required (otherwise "nothing to
  audit").
- `canon` holds **file paths / `file:section` pointers** only — never copy the
  spec's contents (copies breed drift). The agent reads the real source.

You can sanity-check the config without invoking the agent:

```sh
specguard scope     # show the resolved baseline / in-scope areas / skipped areas
specguard prompt    # show the prompt handed to each shard
```

---

## 5. First audit

```sh
specguard run
```

- Each in-scope area + the invariants are audited in parallel, each in a separate
  process (fresh context), then merged.
- Results:
  - `reports/spec-audit/<date>.md` … the report
  - `reports/spec-audit/.last-ref` … next run's change-triggered baseline (advances only on a clean run)
  - `.specguard-pending` … created only when there's a `needs_user=yes` finding (the sentinel)

Once you've handled the findings, clear the sentinel:

```sh
specguard ack
```

> **Important**: while a sentinel is pending, the baseline does not advance (so
> unfixed drift isn't lost). The same scope keeps being re-audited until you
> handle it and `ack`.

### Common flags

```sh
specguard --config path/to/specguard.toml run    # specify the config (-c)
specguard --baseline HEAD~5 run                   # override the baseline (-b)
SPECGUARD_BASELINE_REF=origin/main specguard run  # same, via env var
specguard --date 2026-06-17 run                   # pin the report date (for tests)
```

---

## 5.5 Decision records (ADR) and the D3 audit

You can pin the *reason* for a spec change to the canon commit at that time:

```sh
specguard decide "Single signing path"
# -> creates decisions/<date>-single-signing-path.md (pinned to canon_commit)
```

Edit the generated record's frontmatter:

- `canon:` … the canon pointer this decision governs (`file` / `file:section`)
- `drivers:` … the **refutable reasons** (e.g. "HMAC key rotation requires a single signing path")
- `review_when:` … the condition under which a driver breaks and the rule should be revisited

Subsequent `specguard run`s then perform the **D3 audit**, checking for each
decision (a) whether the canon it points at still matches (freshness), and
(b) whether the driver/review_when still hold (staleness = a rule that outlived
its reason). A decision log is *evidence*, not authority — the canon is always
the source of truth. `[decisions] dir` can point at an in-repo directory or an
Obsidian vault path (`""` disables it).

## 6. Scheduled runs (Human-on-the-loop)

Run `specguard run` from a scheduler and have the SessionStart hook (installed by
`specguard init`) pick up `.specguard-pending` to prompt "start fixing?".

cron example (Linux / WSL2):

```cron
0 9 * * * cd /path/to/your/repo && /home/you/.local/bin/specguard run >> /tmp/specguard.log 2>&1
```

On the Windows Task Scheduler, register `specguard.exe run` with the working
directory set to the repo root.

---

## 7. Exit codes (for scheduler / hook integration)

Not repeated here (copies breed drift). See the
**[exit-codes table in README.md](README.md#exit-codes)**; the source of truth is
the `EXIT_*` constants in `src/main.rs`.

---

## 8. Uninstall

```sh
rm ~/.local/bin/specguard                 # the binary
# on the target repo side (optional):
rm specguard.toml .specguard-pending
rm -r reports/spec-audit
# remove the SessionStart hook from .claude/settings.json by hand
```

---

## 9. Troubleshooting

| Symptom | Cause / fix |
|---|---|
| `specguard: command not found` | `~/.local/bin` isn't on PATH. Do the PATH setup in §2 |
| `spawning agent 'claude'` fails | the `claude` CLI is missing/unauthenticated. Not needed for anything but `run` |
| exit 2 "nothing to audit" | no `[[area]]`/`[[invariant]]` defined. Edit `specguard.toml` |
| `baseline ... failed` → all-tracked | neither baseline nor fallback resolved, so all tracked files are audited (e.g. a young repo's first run). Ignore if intended, else tune `[scope].fallback_ref` |
| sentinel won't clear | run `specguard ack` after handling (a clean run alone doesn't clear it) |
| the agent tries to write and fails | by default the harness enforces read-only (allowlist + auto-deny). Working as intended. **For the first run, do one real `claude` `run` and confirm writes / arbitrary Bash are actually blocked** (permission-flag behavior can vary by CLI version) |

---

## For developers

```sh
cargo test                  # unit + integration (fake agent; no real LLM)
cargo clippy --all-targets
```
