#!/bin/bash
# Yggdrasil (Cardano-node) session-start hook for Claude Code on the web.
#
# Goal: ensure that by the time the agent runs `cargo fmt --all -- --check`,
# `cargo check-all`, `cargo test-all`, or `cargo lint`, the pinned 1.95.0
# toolchain (with clippy + rustfmt) is installed and the workspace
# dependency graph is fetched.
#
# Idempotent: safe to re-run. Non-interactive.
set -euo pipefail

# Run async so web sessions start without waiting on `cargo fetch`. The
# harness reads this first line as hook config, then keeps streaming the
# rest in the background. asyncTimeout is in ms.
echo '{"async": true, "asyncTimeout": 300000}'

# Only run in the remote (web) environment. Local sessions already have
# whatever toolchain the developer chose.
if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "${CLAUDE_PROJECT_DIR:-$(pwd)}"

# Make sure cargo/rustup binaries are on PATH for this session.
if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

if [ -n "${CLAUDE_ENV_FILE:-}" ]; then
  echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> "$CLAUDE_ENV_FILE"
  # Coloured cargo output is fine in the web terminal.
  echo 'export CARGO_TERM_COLOR=always' >> "$CLAUDE_ENV_FILE"
fi

# System packages required by the workspace build (pure-Rust, no FFI crypto,
# but a few crates compile small C shims via build.rs and need a working
# toolchain + pkg-config). On the standard Claude Code on the web image
# these are already present; install if missing and we have sudo.
need_pkgs=()
command -v cc >/dev/null 2>&1 || need_pkgs+=(build-essential)
command -v pkg-config >/dev/null 2>&1 || need_pkgs+=(pkg-config)
if [ "${#need_pkgs[@]}" -gt 0 ]; then
  if command -v sudo >/dev/null 2>&1; then
    sudo DEBIAN_FRONTEND=noninteractive apt-get update -qq
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends "${need_pkgs[@]}"
  elif [ "$(id -u)" = "0" ]; then
    DEBIAN_FRONTEND=noninteractive apt-get update -qq
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends "${need_pkgs[@]}"
  else
    echo "warning: missing packages and no root/sudo available: ${need_pkgs[*]}" >&2
  fi
fi

# Trigger toolchain provisioning. `rust-toolchain.toml` pins channel 1.95.0
# with clippy + rustfmt; any cargo invocation under the workspace causes
# rustup to materialise it, so this also serves as a sanity check.
if command -v rustup >/dev/null 2>&1; then
  rustup show active-toolchain >/dev/null
fi

# Warm the cargo registry / git index. Cheap on a cache hit, ~tens of
# seconds on a fresh container — runs once per cached image.
cargo fetch --locked

echo "session-start: yggdrasil-node ready (rustc $(rustc --version 2>/dev/null || echo '?'))"
