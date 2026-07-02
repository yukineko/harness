# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.3](https://github.com/yukineko/claude-harnesses/releases/tag/budgetguard-v0.1.3) - 2026-07-02

### Added

- *(budgetguard,fugu-router)* budget-aware model downgrade

### Fixed

- *(budgetguard)* reset daily budget at local midnight, not UTC
- *(marketplace)* point repo URLs at yukineko/claude-harnesses
- *(tests,robustness)* drain backlog p2s + fix flaky stdin BrokenPipe in CI
- *(budgetguard)* make the daily ledger update atomic, locked, and corruption-safe

### Other

- rebuild plugin binaries [skip ci]
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
- cargo fmt --all + add CI fmt --check gate
- *(harness-status)* import shared Ledger/SessionRecord instead of mirror structs
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(version)* sync Cargo.toml to plugin.json and gate parity in CI
- *(lint)* fix all clippy warnings and add a clippy -D warnings gate
- rebuild plugin binaries [skip ci]
- *(plugins)* move blocking gates back to Stop (revert 41a8d61 for gates)
- *(gates)* correct descriptions to SessionEnd advisory; bump to 0.1.2
- rebuild plugin binaries [skip ci]
- *(plugins)* bump versions for hook migration + launcher fixes
- *(plugins)* move Stop hooks to SessionEnd
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- distribute from this repo; drop the separate-repo (claude-plugins) plan
- rebuild plugin binaries [skip ci]
- Add fugu-router + finalize harness gap plugins; build new plugins in CI
