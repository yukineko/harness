# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.3](https://github.com/yukineko/claude-harnesses/releases/tag/donegate-v0.1.3) - 2026-07-02

### Added

- *(harness-status,harness-core)* Stop-gate latency observability + contract doc
- *(donegate)* gate project-config commands behind workspace trust

### Fixed

- *(tests,robustness)* drain backlog p2s + fix flaky stdin BrokenPipe in CI
- *(gates)* wrap donegate/reviewgate/precommit-audit hook bodies in a panic guard

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
- *(harness-core)* hoist JSONL log-append into gate::run::append_jsonl
- cargo fmt --all + add CI fmt --check gate
- rebuild plugin binaries [skip ci]
- *(donegate)* thin adapter over harness_core::gate
- rebuild plugin binaries [skip ci]
- *(version)* sync Cargo.toml to plugin.json and gate parity in CI
- rebuild plugin binaries [skip ci]
- *(plugins)* move blocking gates back to Stop (revert 41a8d61 for gates)
- *(gates)* correct descriptions to SessionEnd advisory; bump to 0.1.2
- rebuild plugin binaries [skip ci]
- *(plugins)* bump versions for hook migration + launcher fixes
- *(plugins)* move Stop hooks to SessionEnd
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- Phase C: migrate remaining plugins onto harness-core
- Phase A: integrate imported crates into the workspace
- Add 'crates/donegate/' from commit '1e6e5f5a646da7a0fcf752f9ec9c5e5dc8baab19'
