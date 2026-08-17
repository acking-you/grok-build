#!/usr/bin/env bash
# Idempotent Cloud Agent bootstrap for the Grok Build Rust workspace.
#
# Responsibilities:
#   1. Ensure DotSlash is available so the hermetic `bin/protoc` wrapper (used by
#      proto codegen build scripts) can download and run.
#   2. Prime the Cargo dependency cache from the committed lockfile.
#   3. Warm the build cache by compiling the primary binaries (the `grok` TUI and
#      the slim `grok-runtime`). This validates proto codegen end to end and makes
#      subsequent agent builds fast.
#
# The pinned Rust toolchain (rust-toolchain.toml) is installed automatically by
# rustup on first cargo invocation.
set -euo pipefail

# Cargo/rustup live here on the default Cloud Agent image; make sure they are on
# PATH even when this script runs in a non-login shell.
export PATH="${CARGO_HOME:-/usr/local/cargo}/bin:${HOME}/.cargo/bin:${PATH}"

echo "==> Toolchain versions"
cargo --version
rustc --version

# 1. DotSlash — required so bin/protoc can fetch and execute protoc.
#    Pin the validated release (with --locked) so a later DotSlash publish cannot
#    silently change what this commit installs and break agent startup.
DOTSLASH_VERSION="0.5.7"
if command -v dotslash >/dev/null 2>&1 \
  && dotslash --version 2>/dev/null | grep -qw "$DOTSLASH_VERSION"; then
  echo "==> dotslash $DOTSLASH_VERSION already installed: $(command -v dotslash)"
else
  echo "==> Installing dotslash $DOTSLASH_VERSION"
  cargo install dotslash --version "$DOTSLASH_VERSION" --locked
fi

echo "==> Verifying hermetic protoc via dotslash"
./bin/protoc --version

# 2. Warm the dependency cache (respects Cargo.lock).
echo "==> Fetching Cargo dependencies (locked)"
cargo fetch --locked

# 3. Warm the build cache / validate proto codegen by building the primaries.
#    Full-workspace builds are slow, so target the shipped binaries only.
echo "==> Building primary binaries (grok TUI + grok-runtime)"
cargo build -p xai-grok-pager-bin -p xai-grok-runtime-bin

echo "==> Cloud Agent environment ready"
