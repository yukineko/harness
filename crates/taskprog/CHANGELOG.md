# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/yukineko/claude-harnesses/releases/tag/taskprog-v0.1.1) - 2026-07-02

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
- *(deps)* centralize common deps to workspace.dependencies + drop stray profiles
- rebuild plugin binaries [skip ci]
- cargo fmt --all + add CI fmt --check gate
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(version)* sync Cargo.toml to plugin.json and gate parity in CI
- *(lint)* fix all clippy warnings and add a clippy -D warnings gate
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(plugins)* bump versions for hook migration + launcher fixes
- *(plugins)* move Stop hooks to SessionEnd
- Merge branch 'main' of https://github.com/yukineko/harness
- *(deny)* fix license check — add workspace license + allow MPL-2.0 and Unicode-3.0
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- Add fugu-router + finalize harness gap plugins; build new plugins in CI
