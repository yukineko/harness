# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/yukineko/claude-harnesses/releases/tag/context-governor-v0.1.0) - 2026-07-02

### Added

- *(harness-core,harness-status)* aggregate UserPromptSubmit injection-budget monitoring (ADR 0001 Phase 2)
- *(context-governor)* apply SpecClassifier lanes in rehydrator [backlog b9ab97a7]
- *(context-governor)* bound ledger.jsonl with GC/rotation [backlog bd3e65fe]
- *(context-governor)* enable as a marketplace plugin
- *(injector)* dedup repeated reference injection via ledger seen-state (I6 observe→act)
- *(groomer)* window-pressure-aware groom budget (I6 observe→act)
- *(groomer)* emit Groomed ledger row from to_output on over-budget groom
- *(context-governor)* durable JSONL ledger sink + rollup reader
- *(context-governor)* implement DefaultRehydrator SessionStart restore
- *(context-governor)* implement TranscriptBackingStore round-trip
- *(context-governor)* implement DefaultClassifier and DefaultInjector (Phase 2b)
- *(context-governor)* implement DefaultGroomer — Phase 2 primary size lever
- *(context-governor)* freeze Phase 1 contract (types, traits, hook dispatch)

### Fixed

- *(context-governor,session-insights)* canonicalize cwd for ledger key parity
- *(context-governor)* fail-soft the backing-store open so a hook never breaks a turn
- *(ci,deps)* enroll context-governor in CI build, bump anyhow, fix condukt build-script

### Other

- *(harness-core)* extract shared context ledger_path, kill writer/reader drift
- rebuild plugin binaries [skip ci]
- repo-wide README + docs refresh (EN/JA parity)
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- Merge origin/main into local main (context-governor marketplace + ledger GC)
- *(context-governor)* note ctxrot coexistence in README
- *(context-governor)* document + lock coexistence with ctxrot [backlog 3332a7bd]
- *(context-governor)* add English README.md (parity)
- *(context-governor)* document the action ledger (I6) + rollup, refresh phase status
- *(context-governor)* action-ledger acceptance E2E + integration env-lock retrofit (backlog 4bddfd4a wave 3)
- *(context-governor)* use fd-preopen to survive backing::cleanup race in snapshot tests
- *(context-governor)* serialise all env-mutating unit tests via one shared lock
- Merge condukt/snapshot-emit: emit/rollup action-ledger wiring (backlog 4bddfd4a wave 2)
- Merge condukt/injector-emit: emit/rollup action-ledger wiring (backlog 4bddfd4a wave 2)
- Merge condukt/groomer-emit: emit/rollup action-ledger wiring (backlog 4bddfd4a wave 2)
- *(context-governor)* Phase-2 guard/rehydrator/checkpointer end-to-end
- Merge condukt/checkpointer-impl: implement Phase-2 checkpointer-impl handler (backlog e951c60a)
- Merge condukt/rehydrator-impl: implement Phase-2 rehydrator-impl handler (backlog e951c60a)
- add Japanese README.ja.md (目的/どうして必要か/どう使うか) for all harnesses
