# Build the per-platform binaries the launcher (bin/blastguard) dispatches to.
# Hooks call ${CLAUDE_PLUGIN_ROOT}/bin/blastguard, which execs
# blastguard-<os>-<arch>. `make bins` refreshes both bundled targets; commit them.
#
# Linux cross-build uses cargo-zigbuild (no Docker). One-time setup on macOS:
#   brew install zig
#   cargo install cargo-zigbuild
#   rustup target add x86_64-unknown-linux-gnu

LINUX_TARGET := x86_64-unknown-linux-gnu
# Pin an old glibc floor so the binary runs on a wide range of distros.
LINUX_GLIBC  := 2.17
# Host (macOS) arch for the darwin binary name.
MAC_ARCH     := $(shell uname -m | sed 's/^arm64$$/arm64/;s/^x86_64$$/x86_64/')
# In a cargo workspace, artifacts land in the workspace-root target/, not the
# crate-local one. Resolve it (honoring CARGO_TARGET_DIR) so the cp steps work.
TARGET_DIR   := $(if $(CARGO_TARGET_DIR),$(CARGO_TARGET_DIR),$(dir $(shell cargo locate-project --workspace --message-format plain))target)

.PHONY: bins mac linux test clean

# Refresh both bundled binaries in bin/.
bins: mac linux

# Native macOS build → bin/blastguard-darwin-<arch>.
mac:
	cargo build --release -p blastguard
	cp $(TARGET_DIR)/release/blastguard bin/blastguard-darwin-$(MAC_ARCH)
	@file bin/blastguard-darwin-$(MAC_ARCH)

# Cross-compile Linux x86_64 → bin/blastguard-linux-x86_64.
linux:
	cargo zigbuild --release -p blastguard --target $(LINUX_TARGET).$(LINUX_GLIBC)
	cp $(TARGET_DIR)/$(LINUX_TARGET)/release/blastguard bin/blastguard-linux-x86_64
	@file bin/blastguard-linux-x86_64

test:
	cargo test -p blastguard

clean:
	cargo clean
