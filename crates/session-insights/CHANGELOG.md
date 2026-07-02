# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.2](https://github.com/yukineko/claude-harnesses/releases/tag/session-insights-v0.2.2) - 2026-07-02

### Added

- *(session-insights)* report --context joins context-governor ledger health [backlog 98e9903b]
- *(session-insights)* show per-agent cost breakdown in record notes (DoD d1b0c2f8+d7dbbd0d)
- *(session-insights)* add 注意点/落とし穴 and 要追跡/あとで確認 record sections
- *(session-insights)* implement /record command and update binaries
- *(session-insights)* write record note at Stop in addition to SessionEnd
- *(session-insights)* release v0.2.0 — cross-session backlog integration
- *(session-insights)* cross-session backlog of open issues in Obsidian
- *(session-insights)* pin /record distillation to a fresh Sonnet subagent

### Fixed

- *(context-governor,session-insights)* canonicalize cwd for ledger key parity
- *(tests,robustness)* drain backlog p2s + fix flaky stdin BrokenPipe in CI
- *(session-insights)* drop SessionStart backlog brief hook (b1)
- *(plugins)* restore per-platform launcher scripts (exec format error)
- *(session-insights)* resolve workspace target/ in Makefile cp steps
- *(session-insights)* pin project/cwd to session start in Session::ensure

### Other

- *(harness-core)* extract shared context ledger_path, kill writer/reader drift
- rebuild plugin binaries [skip ci]
- repo-wide README + docs refresh (EN/JA parity)
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- add Japanese README.ja.md (目的/どうして必要か/どう使うか) for all harnesses
- *(bin)* rebuild darwin-arm64 plugin binaries from current source
- rebuild plugin binaries [skip ci]
- *(workspace)* bump MSRV to 1.85, consolidate FNV-1a, harden CI & many crates
- *(session-insights)* cover gauge-absent fallback path in cost_body() (DoD 9da6e2e1)
- rebuild plugin binaries [skip ci]
- *(core)* add store::load_json/save_json and route equivalent state IO through them
- *(core)* consolidate forked HookInput structs onto hook::HookInput
- *(deps)* centralize common deps to workspace.dependencies + drop stray profiles
- rebuild plugin binaries [skip ci]
- cargo fmt --all + add CI fmt --check gate
- *(harness-core)* make SessionRecord the canonical cost source; cut one of the triple-parse
- rebuild plugin binaries [skip ci]
- repoint backlog + document one-time migration (b3+b4)
- repoint backlog references to standalone backlog CLI (b3)
- *(session-insights)* remove standalone backlog store (b2)
- rebuild plugin binaries [skip ci]
- *(version)* sync Cargo.toml to plugin.json and gate parity in CI
- *(lint)* fix all clippy warnings and add a clippy -D warnings gate
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(plugins)* bump versions for hook migration + launcher fixes
- *(plugins)* move Stop hooks to SessionEnd
- *(session-insights)* bump version to 0.2.0 to match plugin.json
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- Add AEGIS-style session record (SessionEnd hook + /record command)
- rebuild plugin binaries [skip ci]
- Phase C: migrate remaining plugins onto harness-core
- Phase A: integrate imported crates into the workspace
- Add 'crates/session-insights/' from commit '28bb6abb7332e8e3db1249602e88c89d8abafec2'
