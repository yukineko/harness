#!/usr/bin/env bash
# mutation-gate.sh — run cargo-mutants on ONE pilot crate and gate on kill-rate.
#
# WHY: golden/regression tests prove the code still behaves as before; they do not
# prove the tests would CATCH a fault. Mutation testing injects small faults and
# checks the suite fails ("kills" the mutant). The fraction of viable mutants
# killed is the kill-rate / mutation score. See Meta ACH and PRIMG
# (arXiv:2505.05584). This script wires the standard tool `cargo-mutants` to the
# deterministic `mutategate` gate (crates/mutategate), which parses outcomes.json,
# computes the kill-rate, and exits non-zero below the threshold.
#
# SCOPE (deliberately narrow — NOT a silent cut):
#   * PILOT = ONE crate only. `cargo-mutants` across the whole workspace is far
#     too slow to gate on; we start with a single small, pure-logic crate.
#     Default pilot: `harness-core` (shared build-time logic; hash/pricing/spans
#     are pure and well-suited to mutation). Override with PILOT=<crate>.
#   * You can narrow further to specific files with MUTANTS_EXTRA="--file src/hash.rs"
#     to keep a real run fast.
#
# HOW TO EXPAND (future work):
#   * Add crates one at a time to a PILOTS list once each holds >= threshold, so a
#     newly-added crate cannot silently drag the gate down.
#   * Raise MIN_KILL_RATE as suites harden. Track survivors from mutants.out/.
#
# THRESHOLD: MIN_KILL_RATE default 0.80. Rationale: 0.80 is the practical
# robustness bar used by established mutation tools (e.g. PIT) and the Meta ACH
# line of work; below it a suite is demonstrably missing detectable faults. Kept
# conservative for the pilot so the gate is signal, not flake; raise over time.
#
# TIME: real mutation runs are slow. This script passes --timeout to bound each
# test build+run; tune MUTANTS_TIMEOUT. CI (.github/workflows/mutation.yml) runs
# pilot-limited with an overall job timeout.
#
# USAGE:
#   scripts/mutation-gate.sh                 # pilot=harness-core, threshold=0.80
#   PILOT=difflog MIN_KILL_RATE=0.7 scripts/mutation-gate.sh
#   MUTANTS_EXTRA="--file src/hash.rs" scripts/mutation-gate.sh
set -euo pipefail

PILOT="${PILOT:-harness-core}"
MIN_KILL_RATE="${MIN_KILL_RATE:-0.80}"
MUTANTS_TIMEOUT="${MUTANTS_TIMEOUT:-120}"
MUTANTS_EXTRA="${MUTANTS_EXTRA:-}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# Make cargo available in non-login shells (rustup layout).
if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

if ! cargo mutants --version >/dev/null 2>&1; then
  echo "mutation-gate: cargo-mutants not found." >&2
  echo "  install with: cargo install cargo-mutants --locked" >&2
  exit 2
fi

out_dir="target/mutants-${PILOT}"
# cargo-mutants nests its results under a `mutants.out/` subdirectory of --output.
outcomes="${out_dir}/mutants.out/outcomes.json"

echo "mutation-gate: running cargo-mutants on pilot crate '${PILOT}' (threshold ${MIN_KILL_RATE})"

# cargo-mutants itself exits non-zero when mutants survive; we want to gate on the
# JSON via mutategate instead, so don't let a survivor abort the script here.
set +e
# shellcheck disable=SC2086
cargo mutants \
  --package "$PILOT" \
  --output "$out_dir" \
  --timeout "$MUTANTS_TIMEOUT" \
  $MUTANTS_EXTRA
mutants_status=$?
set -e

if [ ! -f "$outcomes" ]; then
  echo "mutation-gate: expected results at ${outcomes} but none were written" >&2
  echo "  (cargo-mutants exit status was ${mutants_status})" >&2
  exit 2
fi

# The deterministic gate: parse outcomes.json, compute kill-rate, exit 1 if below.
cargo run --quiet --package mutategate -- \
  --outcomes "$outcomes" \
  --min-kill-rate "$MIN_KILL_RATE"
