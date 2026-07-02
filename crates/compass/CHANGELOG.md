# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2](https://github.com/yukineko/claude-harnesses/compare/compass-v0.0.1...compass-v0.1.2) - 2026-07-02

### Added

- *(compass)* add discovery subcommands + cross-session opportunity dedup
- *(compass,flow)* add pivot-check subcommand + wire into flow Step 4 (DoD#2+#4)
- *(compass)* aggregate outcome streak into a pivot-or-persevere signal (DoD#1+#3)
- *(compass)* rank opportunities by weight desc in gap + handoff (OST load-bearing DoD#2/#3)
- *(compass)* opportunities carry an ordering weight (OST load-bearing DoD#1)
- *(compass)* gap emits per-opportunity gap array under the active outcome (OST DoD#3)
- *(compass)* handoff carries named opportunities under the active outcome (OST DoD#2)
- *(compass)* add opportunity layer (OST) — persisted store + `opportunity add/list`
- *(compass,autoflow)* gate autoflow's backlog auto-drive on compass freshness
- *(compass)* add `outcome` subcommand — record move verdict vs measuring_stick

### Fixed

- *(hooks)* correct 5 plugins' manifest hooks to Stop (was SessionEnd)
- *(discovery)* earliest-owner dedup — stop mutual annihilation + count Selected
- *(tests,robustness)* drain backlog p2s + fix flaky stdin BrokenPipe in CI

### Other

- rebuild plugin binaries [skip ci]
- Merge remote-tracking branch 'origin/main'
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
- cargo fmt --all + add CI fmt --check gate
- *(flow,compass,hypothesis)* sync READMEs with the PDO measurement loop
- rebuild plugin binaries [skip ci]
- *(version)* bump compass 0.1.2, flow 0.1.1 — measurement loop release
- *(flow,compass)* wire `compass outcome` into the sink — close the loop end-to-end
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(version)* sync Cargo.toml to plugin.json and gate parity in CI
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(plugins)* bump versions for hook migration + launcher fixes
- *(plugins)* move Stop hooks to SessionEnd
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- distribute from this repo; drop the separate-repo (claude-plugins) plan
- goal re-grounding → next-move plugin upstream of condukt
