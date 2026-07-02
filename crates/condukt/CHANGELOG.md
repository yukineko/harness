# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.0](https://github.com/yukineko/claude-harnesses/releases/tag/condukt-v0.6.0) - 2026-07-02

### Added

- *(skills)* route condukt/scout/flow gates through `policy answer`
- *(condukt)* non-interactive question shim — `policy answer` self-answers on auto
- *(condukt)* durable checkpoint/journal/rollback — the reversibility safety net (charter #7)
- *(condukt)* wire policy engine — `policy decide` CLI + autonomy-check delegates
- *(condukt)* deterministic autonomy policy core (risk x reversibility x confidence)
- *(condukt)* wire edit-time compile gate to a PostToolUse hook (editgate)
- *(condukt)* deterministic edit-time compile gate core (mirrors F->P oracle)
- *(condukt)* enforce F→P oracle at verified gate
- *(condukt)* add state check-oracle for F→P reproduction gate
- *(condukt)* add Task.kind + requires_fp_oracle() for F→P gate eligibility
- *(condukt)* multi-sample self-consistency voting for generated code
- *(condukt)* enforce verifier!=worker and never auto-skip verifier for behavioral criteria
- *(condukt)* add autonomy switch to gate Phase 3 agreement
- *(condukt)* add state check-criteria subcommand for mechanical done_criteria gate
- *(gauge,condukt,harness-core)* per-sub-agent cost so fugu-router routing holds
- *(condukt)* deterministic fugu-router outcome recording via Stop hook
- *(gauge,condukt)* add session --json flag + wire cost into condukt/fugu-router (DoD 3d5560d8+8e111faa)
- *(condukt)* wire tracekit span recording into Phase 4/6 (feeds replaykit)
- *(replaykit,condukt)* wire trace→replay promote into Phase 8 + seed golden
- *(trajectoryeval,condukt)* register plugin + wire trajectory check into Phase 6
- *(eval,condukt)* wire skill-fingerprint canary into CI + record
- *(curate,evalkit)* promote fugu playbooks into golden datasets; recursive evals
- *(hypothesis)* add awaiting-measurement status set by condukt on merge
- *(condukt)* add experiment task class excluded from the merge path
- *(hypothesis)* require measured evidence to validate/reject; stop condukt auto-validating
- *(condukt)* add `condukt status` command with ASCII tree view
- *(condukt)* integrate deepwiki into Phase 1 and Phase 8; expand USAGE.md
- *(condukt)* add spec-drift check in Phase 8 after gate PASS
- *(condukt)* inject relevant hypotheses into interpreter context; auto-validate on gate PASS
- *(condukt)* integrate hypothesis context into Phase 0-next and Phase 8
- *(condukt)* add Phase 0-next for post-completion 'what's next?' case
- *(condukt)* auto-fill --label in state init from tty or pid fallback
- *(condukt)* add loop config example to init and document condukt loop in README
- *(condukt)* add `condukt loop` subcommand for test-fix cycle iteration
- *(condukt)* add loop core — run_cycle, count_test_failures, loop_should_stop
- *(condukt)* add [loop] config section with ModuleCycle and build/deploy commands
- *(condukt)* add Pause/Resume commands and [paused] display in state list
- *(condukt)* add state abandon command to reset running/failed tasks to pending
- *(condukt-worker)* add stuck-awareness / early-abort guidance
- *(condukt)* add small-task fast path (Phase 4.5.5) to SKILL.md
- *(condukt/SKILL.md)* Phase 5 worker プロンプトをテンプレート表形式に整理
- *(condukt/SKILL.md)* add stuck detection and merge conflict recovery flow
- *(condukt)* report unmerged branch when worktree remove cannot delete it
- *(condukt)* add merge pre-flight conflict detection to worktree::merge
- *(condukt)* add stuck_task_ids detection and stuck_ttl_secs config
- *(condukt)* add None overwrite protection and clear flags for state set
- *(condukt)* add updated_at timestamp to TaskState for stuck detection
- *(condukt)* atomic write for RunState::save and save_decomposition
- *(condukt)* wire fugu-router playbook search/record into SKILL.md
- *(condukt)* add confidence field and knowledge_context reception to interpreter agent
- *(condukt)* add confidence field to Task for self-assessed completability
- *(condukt)* wire knowledge injection, confidence gate, peer-awareness, interface context to SKILL.md
- *(condukt)* add knowledge subcommand for persistent knowledge injection
- *(condukt)* 4 skill improvements for speed, cost, and correctness
- *(condukt)* add state reconcile to auto-detect merged/gone branches
- *(condukt)* add state stats, run resume, and Phase 0-alt
- *(condukt)* add test_command config field (pre-work for state test cmd)
- *(interpreter)* add target_symbols and reproduction_tests fields

### Fixed

- *(condukt)* F→P gate — no-proofs oracle is fallback, not a hard reject
- *(condukt)* add autonomous field to make_test_cfg Config literal
- *(condukt)* propagate orphan detection errors into gate_reasons
- *(condukt)* use recorded branch_sha in reconcile to avoid force-push false positives
- *(ci,deps)* enroll context-governor in CI build, bump anyhow, fix condukt build-script
- *(gauge,condukt)* make Phase 6 cost capture actually work (always-0 bug)
- *(tests,robustness)* drain backlog p2s + fix flaky stdin BrokenPipe in CI
- *(condukt)* add --cost to verifier tracekit span (DoD 2e2cf2fd)
- *(condukt)* sanitize worktree topic/branch (path traversal + git option injection)
- *(plugins)* restore per-platform launcher scripts (exec format error)
- *(condukt)* add linked_hypotheses to interpreter schema; align README
- *(condukt)* add paused: false to RunState test fixtures
- *(condukt)* clarify dry-run, gate-fail flow, interface_context criteria, researcher triggers
- *(condukt)* fix skill flow - allowed-tools, researcher handoff, blocked handling, target_symbols, diff command
- *(condukt)* make verifier model dynamic and add target_symbols to inputs
- *(condukt)* permanent fix for git<->cache asset drift
- *(condukt)* address review findings on agentic-coding additions

### Other

- Merge origin/main into local main (condukt feature union)
- *(condukt)* document F→P reproduction gate in READMEs
- *(condukt)* cargo fmt oracle + state F→P gate code
- repo-wide README + docs refresh (EN/JA parity)
- rebuild plugin binaries [skip ci]
- *(condukt)* assert autonomy stop-invariant (worker-blocked + GATED only)
- *(condukt)* require cargo check before commit in worker
- cargo fmt --all --check (unblock smoke gate)
- *(condukt,specguard)* add 22 proptest invariant tests for schedule and scope
- rebuild plugin binaries [skip ci]
- Merge branch 'main' of https://github.com/yukineko/claude-harnesses into chore/condukt-linux-binary-rebuild
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- add Japanese README.ja.md (目的/どうして必要か/どう使うか) for all harnesses
- *(bin)* rebuild darwin-arm64 plugin binaries from current source
- rebuild plugin binaries [skip ci]
- *(workspace)* bump MSRV to 1.85, consolidate FNV-1a, harden CI & many crates
- rebuild plugin binaries [skip ci]
- *(deps)* centralize common deps to workspace.dependencies + drop stray profiles
- resolve spec-drift sentinel (condukt canon dup + silence, harness-core inject)
- *(schemaguard)* register plugin (marketplace, README, CI) + condukt pre-check
- rebuild plugin binaries [skip ci]
- *(fugu-router)* rename `playbook` subcommand to `procedures`
- cargo fmt --all + add CI fmt --check gate
- *(condukt)* run reproduction_tests deterministically before the LLM verifier
- *(condukt)* default interpreter/researcher to sonnet, escalate to opus only when ambiguous
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- repoint backlog references to standalone backlog CLI (b3)
- rebuild plugin binaries [skip ci]
- *(core)* centralize cross-platform shell invocation in harness_core::shell
- *(version)* sync Cargo.toml to plugin.json and gate parity in CI
- *(core)* unify project_key/fnv1a32/repo_root in harness_core::projkey
- *(lint)* fix all clippy warnings and add a clippy -D warnings gate
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(plugins)* bump versions for hook migration + launcher fixes
- add Japanese internals documentation for condukt
- *(condukt)* add GitHub Issues conflict-check integration design doc
- *(condukt)* rebuild linux binary, fix warnings, add Phase 3.5 to SKILL.md
- *(condukt)* release v0.3.1 — add state pause/resume and [paused] list indicator
- *(condukt-worker)* add knowledge_context and peer_tasks to received information
- *(condukt)* add confidence field to verifier agent output spec
- rebuild plugin binaries [skip ci]
- *(condukt)* bump plugin to 0.3.0 with run-resume, reconcile, stats
- *(condukt)* document stats/reconcile/resume-context and wire reconcile into Phase 7
- *(condukt)* add missing env vars and state test docs to README
- *(condukt)* bump plugin to 0.2.0 with updated description
- rebuild plugin binaries [skip ci]
- goal re-grounding → next-move plugin upstream of condukt
- Add fugu-router + finalize harness gap plugins; build new plugins in CI
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- Phase C: migrate remaining plugins onto harness-core
- Phase A: integrate imported crates into the workspace
- Add 'crates/condukt/' from commit 'f0d024ce76d5ec5b863cdec738b849e00a72c1ab'
