#!/usr/bin/env bash
# Staged-C PoC for DESIGN.md hypothesis 3: worktree-parallel implementation with
# per-task audit gating a conflict-free sequential merge (DESIGN.md §6).
#
#   ②normalize+③ratify each area  →  ⑤ implement each in its OWN git worktree
#   (concurrently)  →  ⑥ audit each worktree (specguard, per-area)  →  merge only
#   the tasks that pass, sequentially. Disjoint areas (one file each) must merge
#   without conflict.
#
#   ./run-parallel.sh         # real loop (calls `claude` — costs tokens)
#   ./run-parallel.sh --dry   # wiring smoke test (no agent calls, no tokens)
set -euo pipefail

DRY=0
[ "${1:-}" = "--dry" ] && DRY=1

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SF="$REPO_ROOT/target/debug/specforge"
SG="$REPO_ROOT/target/debug/specguard"
DATE="2026-06-18"
TASKS=(clamp slug)   # id == area == src/<id>.py, one per disjoint area

if [ ! -x "$SF" ] || [ ! -x "$SG" ]; then
  echo "building binaries…" >&2
  (cd "$REPO_ROOT" && cargo build -q)
fi

WORK="$(mktemp -d)"
trap 'git -C "$WORK" worktree prune 2>/dev/null || true; rm -rf "$WORK"' EXIT
cp -r "$SCRIPT_DIR/canon" "$WORK/"
cp "$SCRIPT_DIR"/req-*.md "$SCRIPT_DIR/specforge.toml" "$SCRIPT_DIR/specguard.toml" "$WORK/"
mkdir -p "$WORK/src"
: > "$WORK/src/.gitkeep"
cd "$WORK"
git init -q
git config user.email poc@poc.poc
git config user.name poc
git config commit.gpgsign false
git add -A
git commit -qm "seed: 2 canons + empty src"
SEED="$(git rev-parse HEAD)"
echo "== scaffolded $WORK (seed $SEED), tasks: ${TASKS[*]} =="

impl_agent() {
  timeout 300 claude --print \
    --allowedTools Read Glob Grep Edit Write "Bash(git diff *)" "Bash(git status *)" \
    --disallowedTools WebFetch
}

if [ "$DRY" = 1 ]; then
  for id in "${TASKS[@]}"; do
    "$SF" --config specforge.toml prompt --id "$id" --req "req-$id.md" --canon "canon/$id.md" >/dev/null
    echo "  specforge prompt ($id): ok"
  done
  "$SG" --config specguard.toml --baseline "$SEED" scope
  echo "✅ DRY wiring smoke test passed."
  exit 0
fi

# --- ②③ draft + ratify each area -------------------------------------------
for id in "${TASKS[@]}"; do
  echo "== ②③ $id: draft + ratify =="
  "$SF" --config specforge.toml --date "$DATE" draft \
    --id "$id" --title "$id" --req "req-$id.md" --canon "canon/$id.md" || true
  [ -f "specs/$id.toml" ] || { echo "❌ ② $id escalated/failed — see .specforge-pending" >&2; cat .specforge-pending 2>/dev/null; exit 1; }
  "$SF" --config specforge.toml --date "$DATE" ratify --id "$id" -m "PoC parallel: $id accepted"
done

# --- ⑤ implement each task in its OWN worktree, CONCURRENTLY ----------------
echo "== ⑤ parallel implement (one worktree per task) =="
for id in "${TASKS[@]}"; do
  git worktree add -q -b "impl-$id" "$WORK/wt-$id" "$SEED"
done
pids=()
for id in "${TASKS[@]}"; do
  (
    cd "$WORK/wt-$id"
    { cat "$SCRIPT_DIR/impl-prompt.md"; printf '\n\n対象 spec: specs/%s.toml\n' "$id"; } | impl_agent >"$WORK/impl-$id.log" 2>&1
    git add -A
    git commit -qm "impl: $id" || true
  ) &
  pids+=($!)
done
for p in "${pids[@]}"; do wait "$p" || true; done
echo "  implemented: $(for id in "${TASKS[@]}"; do git -C "$WORK/wt-$id" show --stat --oneline HEAD | head -1; done | tr '\n' ' ')"

# --- ⑥ per-task audit in each worktree, then merge the clean ones -----------
echo "== ⑥ per-task audit + sequential merge =="
merged=() ; held=()
for id in "${TASKS[@]}"; do
  rm -f "$WORK/wt-$id/.specguard-pending"
  ( cd "$WORK/wt-$id" && "$SG" --config specguard.toml --date "$DATE" --baseline "$SEED" run >/dev/null 2>&1 || true )
  if [ -f "$WORK/wt-$id/.specguard-pending" ]; then
    echo "  ⚠ $id: audit flagged drift — HELD (not merged):"
    sed 's/^/      /' "$WORK/wt-$id/.specguard-pending"
    held+=("$id")
  else
    git merge --no-ff -q -m "merge task $id (audited clean)" "impl-$id"
    echo "  ✅ $id: audit clean → merged"
    merged+=("$id")
  fi
done

# --- final audit on the merged result --------------------------------------
echo "== final ⑥ audit on merged tree =="
rm -f .specguard-pending
"$SG" --config specguard.toml --date "$DATE" --baseline "$SEED" run || true

echo
echo "===================================================================="
echo "hypothesis 3 PoC result:"
echo "  merged (parallel, audited, conflict-free): ${merged[*]:-none}"
echo "  held (failed audit, 差し戻し対象):           ${held[*]:-none}"
echo "  files in merged tree: $(ls src/*.py 2>/dev/null | tr '\n' ' ')"
if [ ! -f .specguard-pending ] && [ "${#held[@]}" -eq 0 ]; then
  echo "  ✅ all tasks merged conflict-free and the merged tree audits clean."
  echo "===================================================================="
  exit 0
fi
echo "  ⚠ some tasks held or merged tree still flags — see above."
echo "===================================================================="
exit 1
