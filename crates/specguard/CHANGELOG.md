# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/yukineko/claude-harnesses/releases/tag/specguard-v0.2.0) - 2026-07-02

### Added

- *(specguard)* emit eprintln warning when MAX_SAMPLE_FILES/MAX_DECISIONS exceeded
- *(specguard)* add specguard testaudit subcommand
- *(specguard)* add scan_repo / collect_mod_graph I/O layer for testaudit
- *(specguard)* gate ack on fix commit presence; add --force bypass
- *(specguard)* add testaudit core - detect skipped tests (pure fn)
- *(specguard)* add pre-compiled binary and fix hook to use CLAUDE_PLUGIN_ROOT
- *(specforge)* add deterministic intake `gather` slice

### Fixed

- *(specguard)* a worker panic must not break the turn in run_shards
- *(specguard)* don't accept self-reported test_result without evidence
- *(specguard,ctxrot)* clear clippy 1.96 lints blocking smoke gate
- *(specguard)* surface read_dir and entry errors in decision.rs
- *(specguard)* fail-safe for inconclusive completeness agent + warn on missing needs_user
- *(specguard)* recover from poisoned mutex in run_shards
- *(tests,robustness)* drain backlog p2s + fix flaky stdin BrokenPipe in CI
- *(plugins)* restore per-platform launcher scripts (exec format error)

### Other

- repo-wide README + docs refresh (EN/JA parity)
- cargo fmt --all --check (unblock smoke gate)
- *(condukt,specguard)* add 22 proptest invariant tests for schedule and scope
- add Japanese README.ja.md (目的/どうして必要か/どう使うか) for all harnesses
- *(bin)* rebuild darwin-arm64 plugin binaries from current source
- *(workspace)* bump MSRV to 1.85, consolidate FNV-1a, harden CI & many crates
- *(deps)* centralize common deps to workspace.dependencies + drop stray profiles
- cargo fmt --all + add CI fmt --check gate
- *(version)* sync Cargo.toml to plugin.json and gate parity in CI
- *(lint)* fix all clippy warnings and add a clippy -D warnings gate
- *(plugins)* bump versions for hook migration + launcher fixes
- *(specguard)* update release binaries
- *(specguard)* add as_str test for TestFindingKind
- *(specguard)* document testaudit and ack fix-commit gate
- update specguard README/OVERVIEW for bundled binary and condukt integration
- track .claude/settings.json + specguard intake design; ignore audit log
- Add fugu-router + finalize harness gap plugins; build new plugins in CI
- Add 'crates/specguard/' from commit '15eb3779c07f27842f1871541a261b42a7c82fe2'
