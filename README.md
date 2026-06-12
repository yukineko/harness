# precommit-audit

A fast, **config-driven pre-commit static audit** that runs the same on
**Linux, macOS, and Windows**. It is a from-scratch Rust port of a PowerShell
pre-commit hook, with all project-specific policy lifted out of the code and into
a TOML file — so the binary itself is generic and reusable across repos.

## What it does

On each invocation it inspects the working set (`git diff HEAD` + untracked
files) and reports findings. Built-in **generic** checks:

| Check | Blocks? | Notes |
|---|---|---|
| Source changed without a test | yes | configurable source / test patterns |
| Hard-coded IP address | yes | RFC 5737 test-nets & loopback are benign |
| Hard-coded secret (`password = "…"`) | yes | env getters / placeholders allowed |
| Swallowed exception / `\|\| true` | yes | bare `except:`, `except … : pass`, … |
| Duplicate function definition | yes (opt-in) | heuristic shared-code reuse via `git grep` |
| `local VAR=$(…)` in a `set -e` script | yes | silent-failure bash footgun |
| Broken Markdown links | yes | repo-relative link targets must resolve |
| CRLF/LF line endings | yes | per-extension policy |
| External linters | yes | py_compile, ruff, bash -n, eslint, tsc, radon, semgrep, gitleaks (each optional) |
| File too long | **warn only** | advisory nudge to split |

**Project-specific** policy is expressed as data-driven `[[rule]]` entries
(regex over added lines, with glob scoping and an allowlist) — never hard-coded.
See [`examples/web-project.toml`](examples/web-project.toml) for a worked config
(node project roots, `console.log` / `print()` rules with glob scoping).

## Install

```sh
cargo install --path .
# or build a release binary:
cargo build --release   # -> target/release/precommit-audit
```

## Usage

```sh
precommit-audit [--mode stop|precommit] [--config <file>] [--root <dir>]
```

- `--mode precommit` — for a pre-commit-framework / git hook on a human commit.
  Exits **1** on failure. Skips the (Claude Code) review contract.
- `--mode stop` (default) — for a Claude Code Stop hook. Honors the subagent
  review contract and exits **2** to feed findings back to the agent.
- `--config` — defaults to `<root>/.precommit-audit.toml`.
- `--root` — defaults to `$CLAUDE_PROJECT_DIR`, else the git top-level.

Exit codes: `0` clean · `1` blocked (precommit) · `2` blocked (stop).

### As a pre-commit-framework hook

```yaml
# .pre-commit-config.yaml
- repo: local
  hooks:
    - id: precommit-audit
      name: precommit-audit
      entry: precommit-audit --mode precommit
      language: system
      pass_filenames: false
```

### As a Claude Code Stop hook

```json
{ "hooks": { "Stop": [ { "hooks": [
  { "command": "precommit-audit --mode stop", "timeout": 30 }
] } ] } }
```

## Configuration

Every knob lives in `.precommit-audit.toml`; all have built-in defaults, so the
file is optional. Start from the documented template
[`.precommit-audit.toml`](.precommit-audit.toml) in this repo.

### Suppression

- Per line: append `# audit-ignore: <reason>` (use `//` for JS/TS). A reason is
  **required** — a bare marker does not suppress.
- Per file: put `audit-ignore-file: <reason>` in the first 20 lines.
- One-shot bypass: create `<audit_dir>/.audit-skip` (consumed on read).

## Why a port

The original hook was PowerShell-only (Windows). This rewrite:

- runs natively on Linux/macOS/Windows as a single static binary,
- is UTF-8 throughout (no CP932 mojibake workarounds),
- separates generic checks (in the binary) from project policy (in TOML),
- ships with an integration test suite (`cargo test`).

## License

MIT
