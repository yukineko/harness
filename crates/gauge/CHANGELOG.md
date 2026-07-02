# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1](https://github.com/yukineko/claude-harnesses/compare/gauge-v0.3.0...gauge-v0.3.1) - 2026-07-02

### Added

- *(gauge,condukt,harness-core)* per-sub-agent cost so fugu-router routing holds
- *(gauge,condukt)* add session --json flag + wire cost into condukt/fugu-router (DoD 3d5560d8+8e111faa)
- *(gauge)* split cache read/write columns and surface hit rate
- *(harness-core,gauge)* attribute cost per agent (main vs sub-agent)

### Fixed

- *(hooks)* correct 5 plugins' manifest hooks to Stop (was SessionEnd)
- *(gauge)* clippy unnecessary_sort_by in subagents_cmd (sort_by_key + Reverse)
- *(gauge,condukt)* make Phase 6 cost capture actually work (always-0 bug)
- *(tests,robustness)* drain backlog p2s + fix flaky stdin BrokenPipe in CI
- *(gauge)* satisfy clippy unnecessary_sort_by (sort_by_key + Reverse)

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
- *(core)* consolidate forked HookInput structs onto hook::HookInput
- *(deps)* centralize common deps to workspace.dependencies + drop stray profiles
- rebuild plugin binaries [skip ci]
- cargo fmt --all + add CI fmt --check gate
- *(gauge)* drop the pricing/transcript re-export shims; call harness_core directly
- *(harness-core)* make SessionRecord the canonical cost source; cut one of the triple-parse
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(version)* sync Cargo.toml to plugin.json and gate parity in CI
- *(lint)* fix all clippy warnings and add a clippy -D warnings gate
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(plugins)* bump versions for hook migration + launcher fixes
- *(plugins)* move Stop hooks to SessionEnd
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- Add AEGIS-style session record (SessionEnd hook + /record command)
- rebuild plugin binaries [skip ci]
- Phase C: migrate remaining plugins onto harness-core
- Phase A: integrate imported crates into the workspace
- Add 'crates/gauge/' from commit '59736964aa2de146afb9b4214a865247d3c15d2a'
