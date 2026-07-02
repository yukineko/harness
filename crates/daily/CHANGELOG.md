# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/yukineko/claude-harnesses/releases/tag/daily-v0.1.0) - 2026-07-02

### Added

- *(plugins)* add missing config for daily/hypothesis and register in marketplace
- *(daily)* add DailyGuard to harness-core and daily crate for SessionStart security audit

### Fixed

- *(marketplace)* point repo URLs at yukineko/claude-harnesses
- *(tests,robustness)* drain backlog p2s + fix flaky stdin BrokenPipe in CI
- *(daily)* surface cargo-deny absence instead of faking a clean audit
- *(ci)* build binaries for all marketplace plugins; add backlog launcher + darwin builds

### Other

- rebuild plugin binaries [skip ci]
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
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- document flow and daily plugins across README/OVERVIEW/USAGE + add crate READMEs
