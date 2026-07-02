# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/yukineko/claude-harnesses/releases/tag/backlog-v0.1.0) - 2026-07-02

### Added

- *(backlog,flow)* wire opportunity weight supply path — `backlog add --weight` + flow handoff (source-layer load-bearing DoD#3)
- *(backlog)* tasks carry an ordering weight; next/list sort by it (source-layer load-bearing DoD#1+#2)
- *(backlog)* show deferred status in fail output and list command
- *(backlog)* call requeue_expired in SessionStart to restore expired deferred tasks
- *(backlog)* implement defer logic in store — mark_failed sets defer_until, next/requeue_expired filter deferred tasks
- *(backlog)* add defer_until field and is_deferred() to Task
- *(backlog)* add .claude-plugin/plugin.json for skill auto-discovery
- *(backlog)* add /backlog loop skill SKILL.md
- *(backlog)* implement full CLI (add/list/next/done/fail/edit/install)
- *(backlog)* add hooks/session_start with cycle instructions
- *(backlog)* add install.rs and plugin.toml (merge conflict resolved)
- *(backlog)* add store.rs with CRUD operations
- *(backlog)* add Cargo.toml, task.rs, config.rs

### Fixed

- *(backlog)* guard requeue_expired against TOCTOU lost-update race
- *(tests,robustness)* drain backlog p2s + fix flaky stdin BrokenPipe in CI
- *(backlog)* close TOCTOU race in lock acquisition
- *(ci)* build binaries for all marketplace plugins; add backlog launcher + darwin builds
- *(core)* make pid liveness and note-write paths cross-platform
- *(backlog)* remove plugin.toml to enable skill auto-discovery; add to marketplace

### Other

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
- rebuild plugin binaries [skip ci]
- *(deps)* centralize common deps to workspace.dependencies + drop stray profiles
- rebuild plugin binaries [skip ci]
- *(backlog)* rustfmt-wrap long assert_eq in weight-ordering test
- rebuild plugin binaries [skip ci]
- cargo fmt --all + add CI fmt --check gate
- rebuild plugin binaries [skip ci]
- *(backlog)* update plugin.json description — loop now in /flow
- *(backlog)* thin-delegate SKILL.md to /flow for driver loop
- rebuild plugin binaries [skip ci]
- *(lint)* fix all clippy warnings and add a clippy -D warnings gate
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- Merge branch 'condukt/t2-skill'
- *(deny)* fix license check — add workspace license + allow MPL-2.0 and Unicode-3.0
