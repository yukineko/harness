#!/usr/bin/env bash
# specguard installer: build the release binary and put it on PATH.
#
#   ./install.sh                 # install to ~/.local/bin
#   SPECGUARD_BIN_DIR=/usr/local/bin ./install.sh
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

echo "specguard: building release binary…"
cargo build --release --manifest-path "$repo_root/Cargo.toml"

mkdir -p "$bin_dir"
install -m 0755 "$repo_root/target/release/specguard" "$bin_dir/specguard"
echo "specguard: installed -> $bin_dir/specguard"

case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) echo "specguard: note — $bin_dir is not on PATH. Add it, e.g.:"
     echo "    echo 'export PATH=\"$bin_dir:\$PATH\"' >> ~/.bashrc && source ~/.bashrc" ;;
esac

echo
echo "next: cd /path/to/your/repo && specguard init"
