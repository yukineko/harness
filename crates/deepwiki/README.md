# deepwiki

A repository **architecture wiki** for Claude Code, generated and kept fresh by a
`/deepwiki` command. Inspired by **Devin Wiki**: instead of re-reading the whole
codebase every time you need orientation, deepwiki writes a concise, source-linked
map of the repo to `.deepwiki/*.md` — committed with the code — and refreshes only
the parts that changed.

Subscription-native: a bundled **Rust scanner** does the deterministic mapping,
a **subagent** writes the pages (so heavy repo reading stays out of your main
conversation), and freshness is tracked against **git**. No API key.

## How it works

```
/deepwiki
```

1. `deepwiki status` — is the wiki fresh vs the current commit?
2. `deepwiki scan` — the Rust binary maps the repo (languages, layout, entry
   points, key files) with no LLM.
3. The **`deepwiki-writer` subagent** turns that map into `.deepwiki/overview.md`
   plus per-module pages, citing real `path:line` locations.
4. `deepwiki stamp` — records the commit so the next run knows what changed.

On later runs `status` reports `✅ fresh` or `⚠ stale` with the changed source
files, and the refresh focuses only on the subsystems that moved.

## The binary

```sh
deepwiki scan            # markdown repo map (--json for machine form)
deepwiki status          # fresh / stale vs the wiki's build commit
deepwiki stamp PAGES…    # record HEAD + the pages written (called by /deepwiki)
deepwiki init            # create .deepwiki/
```

## Layout

- `commands/deepwiki.md` — the `/deepwiki` orchestration command.
- `agents/deepwiki-writer.md` — the page-writing subagent.
- `bin/deepwiki` + `bin/deepwiki-<os>-<arch>` — the bundled scanner.
- `.deepwiki/` (in *your* repo) — the generated wiki + `_meta.toml` freshness stamp.

## Build

```sh
make bins     # refresh bin/deepwiki-darwin-<arch> and bin/deepwiki-linux-x86_64
cargo test
```

The committed `bin/deepwiki-*` binaries are what the plugin ships, so end users
need neither cargo nor an API key. Rebuild and recommit them when behavior
changes.
