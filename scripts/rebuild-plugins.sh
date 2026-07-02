#!/usr/bin/env bash
# Clean-rebuild every workspace binary and refresh the *installed* plugin cache
# for the host platform, so source changes actually take effect at runtime.
#
# Each plugin ships a per-platform binary (<name>-<os>-<arch>) that a bin/<name>
# launcher execs. After editing crate source you must rebuild and swap the
# installed copy under the plugin cache — recompiling the repo alone changes
# nothing the running harness sees. This script does both.
#
# Steps:
#   1. cargo clean         (skip with --no-clean)
#   2. cargo build --release --workspace --bins
#   3. copy target/release/<name> over every matching <name>-<os>-<arch> in the
#      live plugin cache   (and, with --stage-repo, the committed crates/*/bin/)
#
# Only the HOST platform's binaries are touched. macOS binaries must be built on
# a Mac (see scripts/build-plugin-bin.sh for the cross/single-crate staging tool).
#
# Usage:
#   scripts/rebuild-plugins.sh                  # clean + release build + refresh cache
#   scripts/rebuild-plugins.sh --no-clean       # incremental build (skip cargo clean)
#   scripts/rebuild-plugins.sh --stage-repo     # ALSO overwrite committed crates/*/bin
#   scripts/rebuild-plugins.sh --dry-run        # show what would change; build nothing, copy nothing
#   CLAUDE_PLUGIN_CACHE=/path scripts/rebuild-plugins.sh   # override plugin cache root
#
# Env:
#   CLAUDE_PLUGIN_CACHE   plugin cache root (default: ~/.claude/plugins/cache/yukineko)
set -euo pipefail
cd "$(dirname "$0")/.."
REPO="$PWD"

clean=1 stage_repo=0 dry=0
for arg in "$@"; do
  case "$arg" in
    --no-clean)   clean=0 ;;
    --stage-repo) stage_repo=1 ;;
    --dry-run)    dry=1 ;;
    -h|--help)    sed -n '2,30p' "$0"; exit 0 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

CACHE="${CLAUDE_PLUGIN_CACHE:-$HOME/.claude/plugins/cache/yukineko}"
# Ask cargo where it actually puts artifacts rather than assuming
# $REPO/target/release — CARGO_TARGET_DIR or a target-dir override in
# .cargo/config.toml (e.g. redirecting off a full C: drive under WSL) changes
# this without changing where cargo build itself writes.
TARGET_DIR="$(cargo metadata --no-deps --format-version=1 | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
REL="${TARGET_DIR:-$REPO/target}/release"

# host <os>-<arch>, matching the launcher's `uname` dispatch and build-plugin-bin.sh
triple="$(rustc -vV | sed -n 's/^host: //p')"
case "$triple" in
  *apple-darwin*) os=darwin ;;
  *linux*)        os=linux ;;
  *windows*)      os=windows ;;
  *)              os=unknown ;;
esac
case "$triple" in
  x86_64-*)  arch=x86_64 ;;
  aarch64-*) arch=arm64 ;;
  *)         arch=unknown ;;
esac
SUF="$os-$arch"

echo "repo:        $REPO"
echo "build dir:   $REL"
echo "cache:       $CACHE"
echo "host target: $triple  ->  $SUF"
echo "clean:       $([ $clean = 1 ] && echo yes || echo 'no (incremental)')   stage-repo: $([ $stage_repo = 1 ] && echo yes || echo no)   dry-run: $([ $dry = 1 ] && echo yes || echo no)"
echo

if [ ! -d "$CACHE" ]; then
  echo "plugin cache not found: $CACHE" >&2
  echo "set CLAUDE_PLUGIN_CACHE to the correct root and retry." >&2
  exit 1
fi

# --- build -----------------------------------------------------------------
if [ $dry = 1 ]; then
  echo "[dry-run] would run:$([ $clean = 1 ] && echo ' cargo clean;') cargo build --release --workspace --bins"
else
  if [ $clean = 1 ]; then
    echo ">>> cargo clean"
    cargo clean
  fi
  echo ">>> cargo build --release --workspace --bins"
  cargo build --release --workspace --bins
fi
echo

# --- refresh ---------------------------------------------------------------
updated_cache=0 updated_repo=0 missing="" checked=0
shopt -s nullglob
for binfile in "$CACHE"/*/*/bin/*-"$SUF"; do
  checked=$((checked+1))
  base=$(basename "$binfile")     # e.g. condukt-<os>-<arch>
  binname=${base%-$SUF}           # e.g. condukt
  src="$REL/$binname"
  if [ ! -x "$src" ]; then
    missing="$missing $binname"
    continue
  fi
  # 1) live cache copy — what the running harness actually execs
  if ! cmp -s "$src" "$binfile"; then
    if [ $dry = 1 ]; then
      echo "cache  would update $base"
    else
      cp -f "$src" "$binfile"; chmod +x "$binfile"
      echo "cache  updated $base"
    fi
    updated_cache=$((updated_cache+1))
  fi
  # 2) committed repo copy — what /plugin install ships (opt-in via --stage-repo)
  if [ $stage_repo = 1 ]; then
    repofile=$(ls "$REPO"/crates/*/bin/"$base" 2>/dev/null | head -n1 || true)
    if [ -n "$repofile" ] && ! cmp -s "$src" "$repofile"; then
      if [ $dry = 1 ]; then
        echo "repo   would update ${repofile#$REPO/}"
      else
        cp -f "$src" "$repofile"; chmod +x "$repofile"
        echo "repo   updated ${repofile#$REPO/}"
      fi
      updated_repo=$((updated_repo+1))
    fi
  fi
done
shopt -u nullglob

echo "---"
echo "cache bins scanned: $checked | cache updated: $updated_cache$([ $stage_repo = 1 ] && echo " | repo bin updated: $updated_repo")"
if [ -n "$missing" ]; then
  echo "WARNING: no release artifact for:$missing" >&2
  echo "(these cache plugins had a $SUF binary but no matching target/release/<name> — a non-workspace or renamed bin?)" >&2
fi
[ $checked = 0 ] && echo "note: no *-$SUF binaries found under $CACHE (wrong cache root, or no host-platform plugins installed)."
exit 0
