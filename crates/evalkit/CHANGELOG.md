# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/yukineko/claude-harnesses/compare/evalkit-v0.1.0...evalkit-v0.1.1) - 2026-07-02

### Added

- *(eval,condukt)* wire skill-fingerprint canary into CI + record
- *(evalkit)* canary subcommand — diff two golden run results (old vs new)
- *(curate,evalkit)* promote fugu playbooks into golden datasets; recursive evals
- *(evalkit)* add offline golden-regression eval harness + CI eval gate

### Other

- repo-wide README + docs refresh (EN/JA parity)
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- add Japanese README.ja.md (目的/どうして必要か/どう使うか) for all harnesses
- *(bin)* rebuild darwin-arm64 plugin binaries from current source
- rebuild plugin binaries [skip ci]
- *(workspace)* bump MSRV to 1.85, consolidate FNV-1a, harden CI & many crates
- *(deps)* centralize common deps to workspace.dependencies + drop stray profiles
- rebuild plugin binaries [skip ci]
