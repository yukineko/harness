# Build the bundled per-platform binaries the plugin hook ships. hooks/hooks.json
# runs ${CLAUDE_PLUGIN_ROOT}/bin/playbook, which is a committed dispatcher shim
# that execs the matching playbook-<os>-<arch> build — so NEVER overwrite
# bin/playbook with a raw binary (that ships a host-only build to every platform
# and breaks the hook with "Exec format error"). `make bins` refreshes the
# per-platform artifacts; commit them.
#
# Linux cross-build uses cargo-zigbuild (no Docker). One-time setup on macOS:
#   brew install zig
#   cargo install cargo-zigbuild
#   rustup target add x86_64-unknown-linux-gnu

LINUX_TARGET := x86_64-unknown-linux-gnu
# Pin an old glibc floor so the binary runs on a wide range of distros.
LINUX_GLIBC  := 2.17

.PHONY: bins mac linux test clean

# Refresh both bundled binaries in bin/.
bins: mac linux

# Native macOS build → bin/playbook-darwin-<arch> (arm64 or x86_64).
mac:
	cargo build --release
	cp target/release/playbook bin/playbook-darwin-$$(uname -m)
	@file bin/playbook-darwin-$$(uname -m)

# Cross-compile Linux x86_64 → bin/playbook-linux-x86_64.
linux:
	cargo zigbuild --release --target $(LINUX_TARGET).$(LINUX_GLIBC)
	cp target/$(LINUX_TARGET)/release/playbook bin/playbook-linux-x86_64
	@file bin/playbook-linux-x86_64

test:
	cargo test

clean:
	cargo clean
