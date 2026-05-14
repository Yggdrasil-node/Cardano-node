#!/usr/bin/env bash
# run-tools.sh — operator launcher for Yggdrasil's pure-Rust ports of
# the upstream IntersectMBO sister tools.
#
# Mirrors the shape of `.reference-haskell-cardano-node/install/run-node.sh`
# but supports the wider 12-binary surface produced by the workspace's
# `crates/{bech32, cardano-submit-api, cardano-testnet, cardano-tracer,
# db-analyser, db-synthesizer, db-truncater, dmq-node, kes-agent,
# kes-agent-control, snapshot-converter, tx-generator}/` crates.
#
# Each tool's binary name matches upstream exactly (no `yggdrasil-`
# prefix on the binary surface), so an SPO can swap an upstream
# binary for a Yggdrasil binary without changing scripts.
#
# Usage:
#   ./run-tools.sh <tool> [args...]
#   ./run-tools.sh --list
#   ./run-tools.sh --help
#
# Examples:
#   ./run-tools.sh bech32 --help
#   ./run-tools.sh cardano-submit-api --config /path/to/submit-api-config.json
#   ./run-tools.sh kes-agent run -F /path/to/kes-agent-config.toml
#
# By default the script invokes a release-built binary at
# target/release/<tool>. If that binary is missing, the script falls
# back to `cargo run --release --bin <tool> -- <args>`. Set
# YGGDRASIL_TOOLS_USE_DEBUG=1 to use debug builds + `cargo run` in
# debug mode (faster iteration, slower runtime).
#
# R327 status: all 12 binaries currently exit 1 with a "not yet
# implemented (R327 skeleton)" sentinel. Concrete subcommand
# implementations land per the R326–R459 sister-tools port arc;
# see docs/operational-runs/2026-05-09-round-327-twelve-skeleton-crates.md.

set -euo pipefail

# Canonical tool list — must match `Cargo.toml` `[workspace.members]`
# entries under `crates/<tool>/` AND the per-tool `[[bin]] name = "<tool>"`
# declarations. Keep in lockstep with `crates/node/yggdrasil-node/src/upstream_pins.rs`
# `UPSTREAM_PINS` for the sister-tool subset (entries 7-9 today).
TOOLS=(
    bech32
    cardano-submit-api
    cardano-testnet
    cardano-tracer
    db-analyser
    db-synthesizer
    db-truncater
    dmq-node
    kes-agent
    kes-agent-control
    snapshot-converter
    tx-generator
)

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"

print_help() {
    sed -n '2,/^set -euo pipefail/p' "$0" | sed -e 's/^# \{0,1\}//' -e '/^set -euo pipefail/d'
}

print_list() {
    echo "Yggdrasil sister-tool binaries (run via $0 <tool> [args...]):"
    for tool in "${TOOLS[@]}"; do
        local crate_dir="$ROOT_DIR/crates/$tool"
        if [[ -d "$crate_dir" ]]; then
            echo "  $tool"
        else
            echo "  $tool   (crate missing — see plan R326–R459)"
        fi
    done
}

is_known_tool() {
    local needle="$1"
    for tool in "${TOOLS[@]}"; do
        if [[ "$tool" == "$needle" ]]; then
            return 0
        fi
    done
    return 1
}

# -- argument parsing -----------------------------------------------

if [[ $# -eq 0 ]]; then
    print_help
    exit 2
fi

case "$1" in
    -h|--help)
        print_help
        exit 0
        ;;
    --list)
        print_list
        exit 0
        ;;
esac

TOOL="$1"
shift

if ! is_known_tool "$TOOL"; then
    echo "run-tools.sh: unknown tool '$TOOL'" >&2
    echo "Run '$0 --list' to see the 12 sister-tool binaries." >&2
    exit 2
fi

# -- dispatch -------------------------------------------------------

USE_DEBUG="${YGGDRASIL_TOOLS_USE_DEBUG:-0}"

if [[ "$USE_DEBUG" -eq 1 ]]; then
    # Debug build / fast iteration.
    cd "$ROOT_DIR"
    exec cargo run --bin "$TOOL" -- "$@"
fi

# Release build (production deployment).
RELEASE_BIN="$ROOT_DIR/target/release/$TOOL"

if [[ -x "$RELEASE_BIN" ]]; then
    exec "$RELEASE_BIN" "$@"
fi

# Release binary not built yet — fall back to `cargo run --release`,
# which builds the binary on demand. Operators deploying to production
# should pre-build via `cargo build --release --workspace` so the cargo
# fallback is only hit during initial setup or development.
echo "run-tools.sh: $TOOL not built yet at $RELEASE_BIN; building via cargo run --release..." >&2
cd "$ROOT_DIR"
exec cargo run --release --bin "$TOOL" -- "$@"
