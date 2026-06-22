#!/usr/bin/env bash
# Recall eval: does re-anchor actually improve decision recall, and is the gain
# worth the added tokens? Drives the model end-to-end via the `claude` CLI
# (subscription, no API key) — the hook itself never calls an LLM.
#
# Requires: `claude` (Claude Code CLI) and `jq` on PATH.
# Usage:
#   scripts/build-plugin-bin.sh            # or `cargo build --release`
#   eval/run-recall.sh [cases] [filler_chars]
#
# It prints an accuracy table for the OFF (no anchor) vs ON (decision re-surfaced
# at the tail) variants plus the re-anchor added-token cost.
set -euo pipefail
cd "$(dirname "$0")/.."

BIN="${CTXROT_BIN:-target/release/ctxrot}"
CASES="${1:-9}"
FILLER="${2:-8000}"

command -v "$BIN" >/dev/null 2>&1 || { echo "build first: cargo build --release (or set CTXROT_BIN)"; exit 1; }
command -v claude >/dev/null 2>&1 || { echo "needs the \`claude\` CLI on PATH"; exit 1; }
command -v jq >/dev/null 2>&1 || { echo "needs \`jq\` on PATH"; exit 1; }

OUT="$(mktemp -d -t ctxrot-eval-XXXXXX)"
"$BIN" eval gen --out "$OUT" --cases "$CASES" --filler-chars "$FILLER" >/dev/null

RESULTS="$OUT/results.jsonl"
: > "$RESULTS"
for f in "$OUT"/*.on.txt "$OUT"/*.off.txt; do
  base="$(basename "$f")"          # case-00.on.txt
  id="${base%.*.txt}"              # case-00
  variant="${base%.txt}"; variant="${variant##*.}"  # on | off
  echo "  asking $id ($variant)…" >&2
  answer="$(claude -p < "$f" | tr '\n' ' ')"
  jq -nc --arg id "$id" --arg v "$variant" --arg a "$answer" \
    '{id:$id, variant:$v, answer:$a}' >> "$RESULTS"
done

echo
"$BIN" eval score --manifest "$OUT/manifest.json" --results "$RESULTS"
echo
echo "cases + results kept in: $OUT"
