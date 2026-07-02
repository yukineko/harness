# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/yukineko/claude-harnesses/releases/tag/autoflow-v0.1.1) - 2026-07-02

### Added

- *(autoflow)* resume /flow after /compact via PreCompact marker + UserPromptSubmit re-injection
- *(flow,autoflow)* make flow propose state-aware and add autoflow session-start
- *(compass,autoflow)* gate autoflow's backlog auto-drive on compass freshness
- *(autoflow)* split backlog_prompts, change backlog message to /backlog, add sessionstart subcommand
- *(autoflow)* check session-insights backlog after condukt tasks complete
- *(autoflow)* mark tasks running on condukt trigger; revert stale after 2h
- *(autoflow)* condukt loop with auto/ask threshold
- *(autoflow)* add session-end auto-flow gate

### Fixed

- *(tests,robustness)* drain backlog p2s + fix flaky stdin BrokenPipe in CI
- *(autoflow)* stand down while another session holds the backlog lock
- *(ci)* build binaries for all marketplace plugins; add backlog launcher + darwin builds
- *(plugins)* restore per-platform launcher scripts (exec format error)
- *(autoflow)* stop retrying /backlog when skill/command keeps failing

### Other

- rebuild plugin binaries [skip ci]
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
- *(core)* add store::load_json/save_json and route equivalent state IO through them
- *(deps)* centralize common deps to workspace.dependencies + drop stray profiles
- rebuild plugin binaries [skip ci]
- *(autoflow)* drop the dead SessionStart backlog-nudge subcommand
- cargo fmt --all + add CI fmt --check gate
- rebuild plugin binaries [skip ci]
- *(autoflow)* serialize HOME-mutating lock tests to fix flaky race
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(version)* sync Cargo.toml to plugin.json and gate parity in CI
- *(core)* unify project_key/fnv1a32/repo_root in harness_core::projkey
- *(lint)* fix all clippy warnings and add a clippy -D warnings gate
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(plugins)* move blocking gates back to Stop (revert 41a8d61 for gates)
- *(plugins)* move Stop hooks to SessionEnd
- *(deny)* fix license check — add workspace license + allow MPL-2.0 and Unicode-3.0
- *(autoflow)* add plugin manifest, hooks, and linux-x86_64 binary
