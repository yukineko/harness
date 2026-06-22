#!/usr/bin/env bash
# Staged-C PoC driver (DESIGN.md §8): wire the IMPLEMENTED binaries — specforge
# (②normalize + ③ratify) and specguard (⑥ drift audit) — into one loop, filling
# the not-yet-implemented ④prompt/⑤impl gap with a write-enabled agent. Proves
# whether 要望→draft→ratify→impl→再監査 closes.
#
#   ./run-poc.sh         # full real loop (calls `claude` — costs tokens)
#   ./run-poc.sh --dry   # wiring smoke test: scaffold + render prompts + resolve
#                        # scope, NO agent calls, NO tokens.
#   ./run-poc.sh --drift # seed an intentional drift instead of the initial impl,
#                        # to exercise the ⑥→差し戻し→⑤ recovery path (the loop's
#                        # core claim: the audit catches drift and the loop fixes
#                        # it). Calls `claude` for the audit + the fix.
#
# The deterministic harness (scope, prompts, gates, convergence control) is this
# script + the two binaries; the judgment (normalize, implement, audit) is the
# agent. Everything runs in a throwaway temp repo so runs are hermetic.
set -euo pipefail

DRY=0
DRIFT=0
case "${1:-}" in
  --dry) DRY=1 ;;
  --drift) DRIFT=1 ;;
  "") ;;
  *) echo "unknown arg: $1 (use --dry or --drift)" >&2; exit 2 ;;
esac

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SF="$REPO_ROOT/target/debug/specforge"
SG="$REPO_ROOT/target/debug/specguard"
DATE="2026-06-18"
ID="poc"
MAX_FIX=2

if [ ! -x "$SF" ] || [ ! -x "$SG" ]; then
  echo "building binaries…" >&2
  (cd "$REPO_ROOT" && cargo build -q)
fi

# --- scaffold a hermetic target repo ---------------------------------------
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cp -r "$SCRIPT_DIR/canon" "$WORK/"
cp "$SCRIPT_DIR/requirement.md" "$SCRIPT_DIR/specforge.toml" "$SCRIPT_DIR/specguard.toml" "$WORK/"
mkdir -p "$WORK/src"
: > "$WORK/src/.gitkeep"
cd "$WORK"
git init -q
git config user.email poc@poc.poc
git config user.name poc
git config commit.gpgsign false
git add -A
git commit -qm "seed: canon + empty src"
SEED="$(git rev-parse HEAD)"
echo "== target scaffolded at $WORK (seed $SEED) =="

CANON="canon/clamp.md"

# Run a write-enabled agent in the target repo, prompt on stdin.
impl_agent() {
  timeout 300 claude --print \
    --allowedTools Read Glob Grep Edit Write "Bash(git diff *)" "Bash(git status *)" \
    --disallowedTools WebFetch || return $?
}

if [ "$DRY" = 1 ]; then
  echo "== DRY: render normalize prompt (no agent) =="
  "$SF" --config specforge.toml prompt --id "$ID" --req requirement.md --canon "$CANON" >/dev/null
  echo "  specforge prompt: ok"
  echo "== DRY: resolve specguard scope + render audit prompt (no agent) =="
  # No src change yet, so force scope via all-tracked fallback for the smoke test.
  "$SG" --config specguard.toml --baseline "$SEED" scope
  echo "  specguard scope: ok"
  echo "✅ DRY wiring smoke test passed (configs parse, prompts render, scope resolves)."
  exit 0
fi

# --- ② normalize + rigor (real agent) --------------------------------------
echo "== ② specforge draft (normalize + rigor) =="
"$SF" --config specforge.toml --date "$DATE" draft \
  --id "$ID" --title "スコアを許容範囲に収める" --req requirement.md --canon "$CANON" || true

if [ ! -f "specs/$ID.toml" ]; then
  echo "❌ ② escalated (rigor 未達) — no draft produced. See .specforge-pending:" >&2
  cat .specforge-pending 2>/dev/null || true
  echo "(This is a valid HOTL outcome, but the loop cannot proceed without a spec.)"
  exit 1
fi
echo "-- draft spec --"; cat "specs/$ID.toml"

# --- ③ ratify (human consent; auto-supplied in the PoC) --------------------
echo "== ③ specforge ratify =="
"$SF" --config specforge.toml --date "$DATE" ratify --id "$ID" \
  -m "PoC: acceptance criteria reviewed and accepted"

# --- ④⑤ implement -----------------------------------------------------------
if [ "$DRIFT" = 1 ]; then
  # Seed a deliberately wrong implementation: clamps to 0..99, violating canon
  # R-HIGH (n>100 -> 100) and R-MID (100 unchanged). The audit must catch this
  # and the recovery loop must fix it. (This tests the harness's recovery path,
  # not the agent's first-pass accuracy.)
  echo "== ④⑤ SEED intentional drift (clamps to 0..99 — violates R-HIGH/R-MID) =="
  cat > src/clamp.py <<'PY'
"""WRONG ON PURPOSE: clamps to 0..99, drifting from canon/clamp.md."""


def clamp_score(n):
    if n < 0:
        return 0
    if n > 99:
        return 99
    return n
PY
  git add -A
  git commit -qm "impl: seeded drift (clamps to 0..99)"
else
  echo "== ④⑤ implement (agent, write-enabled) =="
  cat "$SCRIPT_DIR/impl-prompt.md" | impl_agent
  git add -A
  git commit -qm "impl: clamp_score" || { echo "❌ impl produced no changes" >&2; exit 1; }
fi
echo "-- implementation --"; cat src/clamp.py 2>/dev/null || echo "(no src/clamp.py!)"

# --- ⑥ audit + 差し戻し loop (real agent) ----------------------------------
RESULT=nonconverge
fix=0
while :; do
  echo "== ⑥ specguard run (drift audit), iteration $fix =="
  "$SG" --config specguard.toml --date "$DATE" --baseline "$SEED" run || true
  if [ ! -f .specguard-pending ]; then
    echo "✅ CONVERGED — no drift after $fix fix iteration(s)."
    RESULT=converged
    break
  fi
  echo "⚠ drift flagged:"; cat .specguard-pending
  if [ "$fix" -ge "$MAX_FIX" ]; then
    echo "❌ did not converge within $MAX_FIX fix iteration(s)."
    break
  fi
  fix=$((fix + 1))
  echo "== 差し戻し → ④⑤ fix iteration $fix =="
  { cat "$SCRIPT_DIR/impl-prompt.md"; printf '\n\n--- 差し戻し ---\n前回監査で drift。次の report を読み、指摘点だけ最小修正せよ: reports/%s.md\n' "$DATE"; } | impl_agent
  git add -A
  git commit -qm "fix: drift iteration $fix" || true
  "$SG" --config specguard.toml ack >/dev/null || true
done

echo
echo "===================================================================="
echo "PoC result: $RESULT"
echo "  target repo: $WORK (removed on exit)"
echo "  stages exercised: ② normalize+rigor / ③ ratify / ④⑤ impl / ⑥ audit+差し戻し"
echo "===================================================================="
[ "$RESULT" = converged ]
