# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2](https://github.com/yukineko/claude-harnesses/compare/playbook-v0.1.1...playbook-v0.1.2) - 2026-07-02

### Added

- *(harness-core,harness-status)* aggregate UserPromptSubmit injection-budget monitoring (ADR 0001 Phase 2)

### Fixed

- *(playbook)* exempt always/normative notes from char-budget eviction
- *(tests,robustness)* drain backlog p2s + fix flaky stdin BrokenPipe in CI
- *(playbook)* bump to 0.1.1 and make launcher shim durable
- *(playbook)* restore platform-dispatch launcher shim for bin/playbook

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
- *(harness-core,playbook,runbook)* hoist shared injection substrate
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- Phase C: migrate remaining plugins onto harness-core
- Phase A: integrate imported crates into the workspace
- Add 'crates/playbook/' from commit '17a0ee84c826ad01c3b87ea9cef7aee0c90ec18c'
