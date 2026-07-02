# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.4](https://github.com/yukineko/claude-harnesses/compare/flow-v0.1.3...flow-v0.1.4) - 2026-07-02

### Added

- *(skills)* route condukt/scout/flow gates through `policy answer`
- *(flow)* mark the selected move in the shared discovery store (discovered -> selected)
- *(compass,flow)* add pivot-check subcommand + wire into flow Step 4 (DoD#2+#4)
- *(hypothesis,flow)* wire confidence to the CLI + flow open-pick (discovery-layer load-bearing DoD#3)
- *(backlog,flow)* wire opportunity weight supply path — `backlog add --weight` + flow handoff (source-layer load-bearing DoD#3)
- *(hypothesis,flow)* add RAT gate — de-risk the leap-of-faith assumption before a full build
- *(flow)* drive the measure step on awaiting-measurement hypotheses
- *(flow)* add hypothesis as a third source (PDO loop entry)
- *(flow)* unified source→executor driver plugin

### Fixed

- *(marketplace)* point repo URLs at yukineko/claude-harnesses
- *(ci)* build binaries for all marketplace plugins; add backlog launcher + darwin builds

### Other

- Merge origin/main into local main (condukt feature union)
- Merge branch 'condukt/t4-flow-skill'
- rebuild plugin binaries [skip ci]
- add Japanese README.ja.md (目的/どうして必要か/どう使うか) for all harnesses
- *(bin)* rebuild darwin-arm64 plugin binaries from current source
- rebuild plugin binaries [skip ci]
- *(workspace)* bump MSRV to 1.85, consolidate FNV-1a, harden CI & many crates
- *(flow,compass,hypothesis)* sync READMEs with the PDO measurement loop
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- *(version)* bump compass 0.1.2, flow 0.1.1 — measurement loop release
- *(flow,compass)* wire `compass outcome` into the sink — close the loop end-to-end
- rebuild plugin binaries [skip ci]
- rebuild plugin binaries [skip ci]
- document flow and daily plugins across README/OVERVIEW/USAGE + add crate READMEs
