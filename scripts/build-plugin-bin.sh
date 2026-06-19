#!/usr/bin/env bash
# Build the release binary and stage it into bin/ so the plugin is self-contained.
# The committed bin/ctxrot is what `/plugin install` ships to users (no cargo
# needed on their side). Re-run this whenever src/ changes, then commit bin/ctxrot.
#
# NOTE: the committed binary is platform-specific (built for the host triple).
# For multi-platform distribution, build per-target and ship per-OS binaries.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release
mkdir -p bin
cp -f target/release/ctxrot bin/ctxrot
chmod +x bin/ctxrot
echo "staged bin/ctxrot ($(uname -m), $(du -h bin/ctxrot | cut -f1))"
echo "remember: git add bin/ctxrot && commit"
