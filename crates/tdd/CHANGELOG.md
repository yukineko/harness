# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.3](https://github.com/yukineko/claude-harnesses/releases/tag/tdd-v0.1.3) - 2026-07-02

### Added

- *(tdd)* add Fail-to-Pass transition oracle (tdd oracle --task)
- *(tdd)* gate project-config test_cmd behind workspace trust

### Fixed

- *(tests,robustness)* drain backlog p2s + fix flaky stdin BrokenPipe in CI

### Other

- rebuild plugin binaries [skip ci]
- Merge origin/main (CI binary rebuild b335252) into F→P gate work
- *(condukt)* e2e coverage for F→P reproduction gate
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
- *(tdd)* thin adapter over harness_core::gate + add panic guard
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
- test-first gate plugin (Stop hook + /tdd skill + RED→GREEN proof)
