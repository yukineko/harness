# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/yukineko/claude-harnesses/releases/tag/fugu-router-v0.1.1) - 2026-07-02

### Added

- *(harness-core,harness-status)* aggregate UserPromptSubmit injection-budget monitoring (ADR 0001 Phase 2)
- *(fugu-router)* bias routing cheap-first with cascade as safety net
- *(condukt)* deterministic fugu-router outcome recording via Stop hook
- *(fugu-router)* cost-adjusted Thompson sampling in decide_bandit() (DoD ab371c1b)
- *(fugu-router)* incorporate cost-per-success into model selection (DoD 5a21a090)
- *(eval,condukt)* wire skill-fingerprint canary into CI + record
- *(fugu-router)* skill_fingerprint on Episode + record arg + fingerprint subcommand
- *(fugu-router)* add `label` subcommand — human teacher signal over self-pass
- *(budgetguard,fugu-router)* budget-aware model downgrade
- *(fugu-router)* judge triviality before the file-count prior
- *(fugu-router)* add sync subcommand for git-based record repository
- *(fugu-router)* add sync-readiness enhancements (playbook_file, import, path normalisation)
- *(fugu-router)* add playbook record/search commands
- *(fugu-router)* add Playbook type and load_playbooks/append_playbook to store
- *(fugu-router)* add playbook_path() to Config (~/.fugu-router/playbooks.jsonl)

### Fixed

- *(tests,robustness)* drain backlog p2s + fix flaky stdin BrokenPipe in CI
- *(fugu-router)* capture git commit/push stderr for diagnosis; add --no-verify
- *(fugu-router)* use git add -u in sync to include all tracked changes

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
- *(fugu-router)* rename `playbook` subcommand to `procedures`
- cargo fmt --all + add CI fmt --check gate
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- Merge branch 'main' of https://github.com/yukineko/harness
- *(deny)* fix license check — add workspace license + allow MPL-2.0 and Unicode-3.0
- *(fugu-router)* add traversal safety and proptest idempotency tests for normalise_path
- *(fugu-router)* tighten path-normalisation assertions to exact equality
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(fugu-router)* add Thompson sampling explainer
- Add fugu-router + finalize harness gap plugins; build new plugins in CI
