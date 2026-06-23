#!/usr/bin/env bash
# specguard installer: build the release binaries and put them on PATH.
#
#   ./install.sh                 # install to ~/.local/bin
#   SPECGUARD_BIN_DIR=/usr/local/bin ./install.sh
#
# Installs BOTH binaries from this crate:
#   - specguard  (audit side: spec↔impl drift)
#   - specforge  (generation side: normalize → ratify → impl-prompt → implement
#                 → evidence → agree → merge; the ②④⑤⑥⑦ pipeline)
#
# After installing, scaffold specguard into a repo:
#   cd /path/to/your/repo && specguard init
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
bin_dir="${SPECGUARD_BIN_DIR:-$HOME/.local/bin}"

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo (Rust toolchain) not found. Install from https://rustup.rs" >&2
  exit 1
fi

echo "specguard: building release binaries (specguard + specforge)…"
cargo build --release --manifest-path "$repo_root/Cargo.toml"

mkdir -p "$bin_dir"
install -m 0755 "$repo_root/target/release/specguard" "$bin_dir/specguard"
echo "specguard: installed -> $bin_dir/specguard"
install -m 0755 "$repo_root/target/release/specforge" "$bin_dir/specforge"
echo "specforge: installed -> $bin_dir/specforge"

case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) echo "specguard: note — $bin_dir is not on PATH. Add it, e.g.:"
     echo "    echo 'export PATH=\"$bin_dir:\$PATH\"' >> ~/.bashrc && source ~/.bashrc" ;;
esac

echo
echo "next: cd /path/to/your/repo && specguard init"
