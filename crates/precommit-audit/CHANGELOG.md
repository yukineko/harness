# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.4](https://github.com/yukineko/claude-harnesses/releases/tag/precommit-audit-v0.1.4) - 2026-07-02

### Added

- *(precommit-audit)* trust-gate auto-discovered project config

### Fixed

- *(precommit-audit)* surface blocking findings in SessionEnd advisory mode
- *(tests,robustness)* drain backlog p2s + fix flaky stdin BrokenPipe in CI
- *(gates)* wrap donegate/reviewgate/precommit-audit hook bodies in a panic guard
- *(precommit-audit)* don't fail the SessionEnd hook on blocking findings

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
- *(precommit-audit)* document why it's not a shared JSON Stop-gate
- cargo fmt --all + add CI fmt --check gate
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(version)* sync Cargo.toml to plugin.json and gate parity in CI
- *(lint)* fix all clippy warnings and add a clippy -D warnings gate
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(plugins)* move blocking gates back to Stop (revert 41a8d61 for gates)
- *(precommit-audit)* bump to 0.1.2 for SessionEnd advisory fix
- rebuild plugin binaries [skip ci]
- *(plugins)* bump versions for hook migration + launcher fixes
- *(plugins)* move Stop hooks to SessionEnd
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- Phase C: migrate remaining plugins onto harness-core
- Phase A: integrate imported crates into the workspace
- Add 'crates/precommit-audit/' from commit 'ca8d4bc7f1cb8f160cf92320c5f17748df0f3f5b'
