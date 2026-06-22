# Build the bundled binaries the plugin hook ships (hooks/hooks.json runs
# ${CLAUDE_PLUGIN_ROOT}/bin/playbook). `make bins` refreshes both; commit them.
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

# Native macOS build → bin/playbook (host arch).
mac:
	cargo build --release
	cp target/release/playbook bin/playbook
	@file bin/playbook

# Cross-compile Linux x86_64 → bin/playbook-linux-x86_64.
linux:
	cargo zigbuild --release --target $(LINUX_TARGET).$(LINUX_GLIBC)
	cp target/$(LINUX_TARGET)/release/playbook bin/playbook-linux-x86_64
	@file bin/playbook-linux-x86_64

test:
	cargo test

clean:
	cargo clean
