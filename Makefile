# Build the per-platform binaries the launcher (bin/session-insights) dispatches
# to. `make bins` refreshes both bundled targets; commit them.
#
# Linux cross-build uses cargo-zigbuild (no Docker). One-time setup on macOS:
#   brew install zig
#   cargo install cargo-zigbuild
#   rustup target add x86_64-unknown-linux-gnu

LINUX_TARGET := x86_64-unknown-linux-gnu
LINUX_GLIBC  := 2.17
MAC_ARCH     := $(shell uname -m | sed 's/^arm64$$/arm64/;s/^x86_64$$/x86_64/')
NAME         := session-insights

.PHONY: bins mac linux test clean

bins: mac linux

mac:
	cargo build --release
	cp target/release/$(NAME) bin/$(NAME)-darwin-$(MAC_ARCH)
	@chmod +x bin/$(NAME) bin/$(NAME)-darwin-$(MAC_ARCH)
	@file bin/$(NAME)-darwin-$(MAC_ARCH)

linux:
	cargo zigbuild --release --target $(LINUX_TARGET).$(LINUX_GLIBC)
	cp target/$(LINUX_TARGET)/release/$(NAME) bin/$(NAME)-linux-x86_64
	@chmod +x bin/$(NAME)-linux-x86_64
	@file bin/$(NAME)-linux-x86_64

test:
	cargo test

clean:
	cargo clean
