# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0](https://github.com/yukineko/claude-harnesses/releases/tag/ctxrot-v0.5.0) - 2026-07-02

### Added

- *(harness-core,harness-status)* aggregate UserPromptSubmit injection-budget monitoring (ADR 0001 Phase 2)
- *(ctxrot)* time the auto-compact nudge off ctxrot's own usage meter
- *(ctxrot)* gate toolguard nudge on per-session seen-state + cap
- *(ctxrot)* add fail-soft per-session nudge_state reader
- *(ctxrot)* add toolguard_nudge_cap config field + env override
- *(ctxrot)* auto-distill at the 200k danger band instead of only nudging /compact
- *(ctxrot)* default distill_on_compact to true (0.4.1)
- *(ctxrot)* add preferred_note / use-note / clear-note

### Fixed

- *(ctxrot)* durably write the distill re-inject marker (temp+fsync+rename)
- *(specguard,ctxrot)* clear clippy 1.96 lints blocking smoke gate
- *(marketplace)* point repo URLs at yukineko/claude-harnesses
- *(tests,robustness)* drain backlog p2s + fix flaky stdin BrokenPipe in CI

### Other

- rebuild plugin binaries [skip ci]
- repo-wide README + docs refresh (EN/JA parity)
- rebuild plugin binaries [skip ci]
- cargo fmt --all --check (unblock smoke gate)
- *(ctxrot)* integrate per-key nudge dedup from main + truncate from branch
- rebuild plugin binaries [skip ci]
- *(context-governor)* document + lock coexistence with ctxrot [backlog 3332a7bd]
- rebuild plugin binaries [skip ci]
- add Japanese README.ja.md (目的/どうして必要か/どう使うか) for all harnesses
- *(bin)* rebuild darwin-arm64 plugin binaries from current source
- rebuild plugin binaries [skip ci]
- *(workspace)* bump MSRV to 1.85, consolidate FNV-1a, harden CI & many crates
- rebuild plugin binaries [skip ci]
- *(core)* add store::load_json/save_json and route equivalent state IO through them
- *(deps)* centralize common deps to workspace.dependencies + drop stray profiles
- rebuild plugin binaries [skip ci]
- cargo fmt --all + add CI fmt --check gate
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(core)* centralize cross-platform shell invocation in harness_core::shell
- *(version)* sync Cargo.toml to plugin.json and gate parity in CI
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- distribute from this repo; drop the separate-repo (claude-plugins) plan
- rebuild plugin binaries [skip ci]
- rebuild macOS binaries with distill-on-compact
- async LLM distill on compaction (distill_on_compact)
- rebuild plugin binaries [skip ci]
- ctxrot v0.4.0: context load control (rule gate + /ctx + switchable carryover)
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- Phase B: extract harness-core; migrate ctxrot onto it
- Phase A: integrate imported crates into the workspace
- Add 'crates/ctxrot/' from commit '77c999d688dbccc8730b0716f0111a3825ee3a3d'
