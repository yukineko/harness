# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`playbook` is a single-binary Rust CLI that doubles as a Claude Code **UserPromptSubmit** hook. It curates "atomic notes" (one fact each) into a local store and, on every prompt, scores those notes against the prompt text and injects the most relevant ones as added context — deterministically, with no embeddings and no API key. It ships as a Claude Code plugin (`.claude-plugin/`) with a prebuilt binary in `bin/`.

## Build / test / run

```sh
cargo build --release            # binary at target/release/playbook
cargo test                       # unit tests live inline (#[cfg(test)]) in retrieve.rs and install.rs
cargo test budget_caps_selection # run one test by name
cargo clippy --all-targets
```

The `[profile.release]` is tuned for a tiny binary (`opt-level = "z"`, `lto`, `strip`). The committed `bin/playbook` and `bin/playbook-linux-x86_64` are the artifacts the plugin hook actually invokes (`hooks/hooks.json` runs `${CLAUDE_PLUGIN_ROOT}/bin/playbook inject`) — **rebuild and refresh those binaries when you change behavior the plugin relies on**, otherwise the installed hook runs stale code.

### Refreshing the bundled binaries

```sh
make bins     # refresh both bin/playbook (host) and bin/playbook-linux-x86_64
make mac      # just the native macOS binary
make linux    # just the Linux x86_64 cross-build
```

The Linux artifact is cross-compiled from macOS with **cargo-zigbuild** (no Docker), pinned to an old glibc floor (`x86_64-unknown-linux-gnu.2.17`) so it runs across distros. One-time setup: `brew install zig && cargo install cargo-zigbuild && rustup target add x86_64-unknown-linux-gnu`. (On rustc < 1.88, pin `cargo install cargo-zigbuild --version 0.21.8`.)

## Exercising the hook locally

`inject` reads a JSON `HookInput` on stdin and prints injected context to stdout. To test retrieval without wiring the hook:

```sh
echo '{"cwd":"'$PWD'","prompt":"lightgbm が OOM で落ちる"}' | cargo run -- inject
cargo run -- search lightgbm が OOM   # shows per-note scores + which would be injected (✓)
cargo run -- status                   # resolved config + store paths + visible note count
PLAYBOOK_DISABLE=1 ...                 # kill switch; inject becomes a no-op
```

## Architecture

The data flow for `inject` (the hook path) is the spine of the program — everything else is curation tooling around the same store and config:

`main::inject` → `Config::load(cwd)` → `Store::load_visible(root)` → `retrieve::select(...)` → `retrieve::render_injection(...)` → stdout.

- **`model.rs`** — `HookInput`, the serde struct for the UserPromptSubmit stdin payload (`cwd`, `prompt`, …).
- **`config.rs`** — layered config: project `./playbook.toml` **over** `~/.playbook/config.toml` **over** built-in defaults (these are not merged — the first file that exists wins). `PLAYBOOK_DISABLE` env kill switch. Store defaults to `~/.playbook/store`.
- **`store.rs`** — the note store. Notes are markdown files with **TOML frontmatter fenced by `+++`** (not YAML — parsed via the `toml` crate; a file with no fence is treated as all-body). Project notes live under `<store>/<basename>-<hash8>/` where the hash of the absolute path disambiguates same-named directories; shared notes under `<store>/_global/`. `slugify` is Unicode-aware so Japanese titles produce readable, distinct slugs.
- **`retrieve.rs`** — the scoring core and the part most worth understanding before changing behavior:
  - `tokenize` lowercases ASCII words (len ≥ 2) and indexes **CJK per-character** (plus a stopword list spanning English and Japanese particles), so Japanese prompts match.
  - `score` weights overlap: `triggers` ×5 > `tags` ×3 > title words ×2 > body overlap (capped at +4 so a long note can't win on noise).
  - `select` injects `always` notes first (they bypass `min_score` but still respect the char budget), then the top scorers above `min_score` up to `top_k`, stopping at `max_chars`. Sorting is stable by slug for **deterministic** output.
- **`install.rs`** — merges/removes the hook in `~/.claude/settings.json`. Idempotent (identifies its own group by a command containing `"playbook"`, preserves foreign hook groups), backs up before any write. This is for the standalone `cargo install` path; the plugin path uses `hooks/hooks.json` instead.

## Conventions specific to this codebase

- **The hook must never fail a turn.** `inject` runs under `run_hook`, which catches panics and always `exit(0)`; bad/empty stdin, missing store, or no relevant notes all silently produce no output. Preserve this — a knowledge hook that blocks prompts is worse than one that stays quiet.
- Injected output is **Japanese-facing** (`render_injection` and the `init` sample note are written in Japanese). Match that when editing user-visible hook output.
- Note location is authoritative for scope; the `scope`/`created` frontmatter fields are informational only.
