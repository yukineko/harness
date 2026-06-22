#!/usr/bin/env bash
# Build a release binary for one workspace member and stage it into that
# crate's bin/ as <name>-<os>-<arch> — the name the bin/<name> launcher looks
# for. The committed per-platform binaries are what `/plugin install` ships, so
# end users need neither cargo nor an API key.
#
# Usage:
#   scripts/build-plugin-bin.sh <plugin>                 # build for the host platform
#   scripts/build-plugin-bin.sh <plugin> <rust-target>   # cross-build, e.g. x86_64-apple-darwin
#   scripts/build-plugin-bin.sh <plugin> <rust-target> <bin-name>   # if bin != crate name
#
# Run from the workspace root. To produce macOS binaries, run this ON A MAC.
set -euo pipefail
cd "$(dirname "$0")/.."

plugin="${1:?usage: build-plugin-bin.sh <plugin> [rust-target] [bin-name]}"
target="${2:-}"
binname="${3:-$plugin}"

if [ -n "$target" ]; then
  rustc_triple="$target"
  cargo build --release -p "$plugin" --target "$target"
  src="target/$target/release/$binname"
else
  rustc_triple="$(rustc -vV | sed -n 's/^host: //p')"
  cargo build --release -p "$plugin"
  src="target/release/$binname"
fi

# normalize <triple> -> <os>-<arch>
case "$rustc_triple" in
  *apple-darwin*) os=darwin ;;
  *linux*)        os=linux ;;
  *windows*)      os=windows ;;
  *)              os="unknown" ;;
esac
case "$rustc_triple" in
  x86_64-*)  arch=x86_64 ;;
  aarch64-*) arch=arm64 ;;
  *)         arch="unknown" ;;
esac

dest="crates/$plugin/bin"
mkdir -p "$dest"
out="$dest/$binname-$os-$arch"
cp -f "$src" "$out"
chmod +x "$out" "$dest/$binname" 2>/dev/null || chmod +x "$out"
echo "staged $out ($(du -h "$out" | cut -f1), from $rustc_triple)"
echo "remember: git add $dest && git update-index --chmod=+x $out && commit"
