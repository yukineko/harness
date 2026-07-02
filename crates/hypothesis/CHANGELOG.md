# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/yukineko/claude-harnesses/compare/hypothesis-v0.1.0...hypothesis-v0.1.1) - 2026-07-02

### Added

- *(hypothesis,flow)* wire confidence to the CLI + flow open-pick (discovery-layer load-bearing DoD#3)
- *(hypothesis)* hypotheses carry a confidence; list sorts by it (discovery-layer load-bearing DoD#1+#2)
- *(hypothesis,flow)* add RAT gate — de-risk the leap-of-faith assumption before a full build
- *(hypothesis)* pre-register success/kill criteria to make bets falsifiable before build
- *(hypothesis)* add awaiting-measurement status set by condukt on merge
- *(hypothesis)* require measured evidence to validate/reject; stop condukt auto-validating
- *(plugins)* add missing config for daily/hypothesis and register in marketplace
- *(hypothesis)* add condukt_run field and --run flag for validate/reject
- *(hypothesis)* add /hypothesis:add skill
- *(hypothesis)* implement SessionStart hook — inject open hypotheses as additionalContext
- *(hypothesis)* implement Store with TOML persistence and atomic save
- *(hypothesis)* implement Config with load/hypotheses_path/disabled_env
- *(hypothesis)* scaffold crate with stub modules

### Fixed

- *(marketplace)* point repo URLs at yukineko/claude-harnesses
- *(tests,robustness)* drain backlog p2s + fix flaky stdin BrokenPipe in CI
- *(hypothesis)* honor list --status filter (was a no-op)
- *(ci)* build binaries for all marketplace plugins; add backlog launcher + darwin builds
- *(hypothesis)* eliminate dead_code warnings — use Hypothesis::new in store, allow Status API methods
- *(hypothesis)* align store/goal_link with Status enum from domain-model

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
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- cargo fmt --all + add CI fmt --check gate
- *(flow,compass,hypothesis)* sync READMEs with the PDO measurement loop
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(lint)* fix all clippy warnings and add a clippy -D warnings gate
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- Merge branch 'condukt/plugin-manifest-skill'
- Merge branch 'condukt/install'
- Merge branch 'condukt/compass-link'
- Merge branch 'condukt/store'
