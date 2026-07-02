# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/yukineko/claude-harnesses/releases/tag/blastguard-v0.1.0) - 2026-07-02

### Added

- *(blastguard)* add PreToolUse hook that denies destructive ops

### Fixed

- *(specguard)* validate LLM-generated test_cmd via blastguard before sh -c
- *(blastguard)* close shell-eval guard bypasses
- *(tests,robustness)* drain backlog p2s + fix flaky stdin BrokenPipe in CI

### Other

- repo-wide README + docs refresh (EN/JA parity)
- *(blastguard)* e2e coverage for shell-eval guard bypasses
- add Japanese README.ja.md (目的/どうして必要か/どう使うか) for all harnesses
- *(bin)* rebuild darwin-arm64 plugin binaries from current source
- *(workspace)* bump MSRV to 1.85, consolidate FNV-1a, harden CI & many crates
- *(deps)* centralize common deps to workspace.dependencies + drop stray profiles
- cargo fmt --all + add CI fmt --check gate
- *(blastguard)* bundle darwin-x86_64 and linux-x86_64 binaries
- *(blastguard)* bundle darwin-arm64 binary
