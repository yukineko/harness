# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2](https://github.com/yukineko/claude-harnesses/compare/beacon-v0.1.1...beacon-v0.1.2) - 2026-07-02

### Added

- *(beacon)* gate project-config command behind workspace trust

### Fixed

- *(hooks)* correct 5 plugins' manifest hooks to Stop (was SessionEnd)
- *(tests,robustness)* drain backlog p2s + fix flaky stdin BrokenPipe in CI

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
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(core)* centralize cross-platform shell invocation in harness_core::shell
- *(version)* sync Cargo.toml to plugin.json and gate parity in CI
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(plugins)* bump versions for hook migration + launcher fixes
- *(plugins)* move Stop hooks to SessionEnd
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- Phase C: migrate remaining plugins onto harness-core
- Phase A: integrate imported crates into the workspace
- Add 'crates/beacon/' from commit '51bc3e2678250e2238db734b2dc97bb65ebb48ec'
