#!/usr/bin/env bash
set -euo pipefail

# Compare the chain tip reported by yggdrasil-node and the upstream Haskell
# cardano-node, asserting both observe the same {slot, hash}. The Haskell
# cardano-cli tip also reports {block, epoch}; Yggdrasil's compatibility
# command currently leaves those optional.
#
# Designed to be run at sampling checkpoints (15 min / 60 min / 6 h) per
# docs/PARITY_SUMMARY.md "Next Steps" item 2 once both nodes are syncing
# against the same network.
#
# Usage:
#   YGG_SOCK=/var/run/ygg.sock HASKELL_SOCK=/var/run/cardano.sock \
#   NETWORK_MAGIC=764824073 scripts/compare_tip_to_haskell.sh
#
# Or for a watching loop (every 15 min):
#   watch -n 900 'YGG_SOCK=/tmp/ygg.sock HASKELL_SOCK=/tmp/cardano.sock \
#     NETWORK_MAGIC=764824073 scripts/compare_tip_to_haskell.sh'
#
# Exit codes:
#   0  tips match (slot AND hash equal)
#   1  tips diverge (slot or hash differ; full diff printed)
#   2  one or both nodes unreachable, unparseable, or missing required tip fields
#   3  bad invocation (missing required env / tools)

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
YGG_BIN="${YGG_BIN:-$ROOT_DIR/target/debug/yggdrasil-node}"
CARDANO_CLI="${CARDANO_CLI:-cardano-cli}"
YGG_SOCK="${YGG_SOCK:-}"
HASKELL_SOCK="${HASKELL_SOCK:-}"
NETWORK_MAGIC="${NETWORK_MAGIC:-764824073}"  # mainnet default
SNAPSHOT_DIR="${SNAPSHOT_DIR:-/tmp/ygg-tip-snapshots}"

usage() {
  cat <<'EOF'
Usage:
  YGG_SOCK=/path/to/yggdrasil.sock \
  HASKELL_SOCK=/path/to/cardano-node.sock \
  NETWORK_MAGIC=<u32> \
  scripts/compare_tip_to_haskell.sh

  scripts/compare_tip_to_haskell.sh --self-test

Required env:
  YGG_SOCK         Unix socket path of running yggdrasil-node (--socket-path)
  HASKELL_SOCK     Unix socket path of running cardano-node

Optional env:
  YGG_BIN          Default: target/debug/yggdrasil-node
  CARDANO_CLI      Default: cardano-cli (must be on $PATH or absolute)
  NETWORK_MAGIC    Default: 764824073 (mainnet); 1 for preprod, 2 for preview
  SNAPSHOT_DIR     Where to drop tip snapshots on mismatch (default /tmp/ygg-tip-snapshots)

Exit codes:
  0  tips match
  1  tips diverge (hash and/or slot differ; snapshot saved)
  2  either node unreachable, invalid JSON, or missing slot/hash
  3  bad invocation

Behaviour on divergence: the raw JSON outputs from both nodes are saved
under $SNAPSHOT_DIR/<timestamp>/ so the operator can decide whether to
abort, snapshot for forensic diff, or continue. Recommended: rerun the
comparison ~30 s later - transient divergence at slot boundaries can
self-heal as one node catches up to the other.
EOF
}

extract_field() {
  local label="$1"
  local key="$2"
  local required="$3"
  local json="$4"

  if ! command -v python3 >/dev/null 2>&1; then
    echo "ERROR: python3 is required for JSON tip parsing" >&2
    return 2
  fi

  python3 -c '
import json
import sys

label, key, required_text = sys.argv[1], sys.argv[2], sys.argv[3]
required = required_text == "1"
raw = sys.stdin.read()

try:
    data = json.loads(raw)
except Exception as exc:
    print(f"ERROR: {label} tip is not valid JSON: {exc}", file=sys.stderr)
    sys.exit(2)

missing = object()

def find_key(value):
    if isinstance(value, dict):
        if key in value:
            return value[key]
        for child in value.values():
            found = find_key(child)
            if found is not missing:
                return found
    elif isinstance(value, list):
        for child in value:
            found = find_key(child)
            if found is not missing:
                return found
    return missing

value = find_key(data)
if value is missing or value is None or value == "":
    if required:
        print(f"ERROR: {label} tip missing required field {key!r}", file=sys.stderr)
        sys.exit(2)
    sys.exit(0)

if isinstance(value, bool):
    print("true" if value else "false")
else:
    print(value)
' "$label" "$key" "$required" <<<"$json"
}

run_self_test() {
  local ygg_json='{"tip":{"slot":42,"hash":"abc123"}}'
  local haskell_json='{"slot":42,"hash":"abc123","block":7,"epoch":3}'
  local missing_hash='{"tip":{"slot":42}}'

  if [[ "$(extract_field yggdrasil slot 1 "$ygg_json")" != "42" ]]; then
    echo "ERROR: failed to extract nested yggdrasil slot" >&2
    return 3
  fi
  if [[ "$(extract_field yggdrasil hash 1 "$ygg_json")" != "abc123" ]]; then
    echo "ERROR: failed to extract nested yggdrasil hash" >&2
    return 3
  fi
  if [[ "$(extract_field haskell block 0 "$haskell_json")" != "7" ]]; then
    echo "ERROR: failed to extract optional Haskell block" >&2
    return 3
  fi
  if [[ -n "$(extract_field yggdrasil block 0 "$ygg_json")" ]]; then
    echo "ERROR: missing optional field should extract as empty" >&2
    return 3
  fi
  if extract_field yggdrasil hash 1 "$missing_hash" >/dev/null 2>&1; then
    echo "ERROR: missing required hash was accepted" >&2
    return 3
  fi
  if extract_field yggdrasil slot 1 'not-json' >/dev/null 2>&1; then
    echo "ERROR: invalid JSON was accepted" >&2
    return 3
  fi

  echo "[ok] compare_tip_to_haskell self-test passed"
}

if [[ "${1:-}" == "--self-test" ]]; then
  run_self_test
  exit $?
fi

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if [[ -z "$YGG_SOCK" || -z "$HASKELL_SOCK" ]]; then
  echo "ERROR: YGG_SOCK and HASKELL_SOCK are required" >&2
  usage
  exit 3
fi

if [[ ! -x "$YGG_BIN" ]]; then
  echo "ERROR: yggdrasil-node binary not found at $YGG_BIN" >&2
  exit 3
fi

if ! command -v "$CARDANO_CLI" >/dev/null 2>&1; then
  echo "ERROR: cardano-cli not found in PATH (set CARDANO_CLI=/abs/path)" >&2
  exit 3
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "ERROR: python3 is required for JSON tip parsing" >&2
  exit 3
fi

if [[ ! -S "$YGG_SOCK" ]]; then
  echo "ERROR: YGG_SOCK is not a unix socket: $YGG_SOCK" >&2
  exit 2
fi

if [[ ! -S "$HASKELL_SOCK" ]]; then
  echo "ERROR: HASKELL_SOCK is not a unix socket: $HASKELL_SOCK" >&2
  exit 2
fi

# Query Yggdrasil tip via the cardano-cli-compatible subcommand.
if ! ygg_tip_json="$("$YGG_BIN" cardano-cli query-tip \
  --socket-path "$YGG_SOCK" \
  --network-magic "$NETWORK_MAGIC" 2>/dev/null)"; then
  echo "ERROR: failed to read tip from yggdrasil-node at $YGG_SOCK" >&2
  exit 2
fi

if [[ -z "$ygg_tip_json" ]]; then
  echo "ERROR: failed to read tip from yggdrasil-node at $YGG_SOCK" >&2
  exit 2
fi

# Query Haskell tip via cardano-cli.
if [[ "$NETWORK_MAGIC" == "764824073" ]]; then
  haskell_net_arg=(--mainnet)
else
  haskell_net_arg=(--testnet-magic "$NETWORK_MAGIC")
fi

if ! haskell_tip_json="$(CARDANO_NODE_SOCKET_PATH="$HASKELL_SOCK" \
  "$CARDANO_CLI" query tip "${haskell_net_arg[@]}" 2>/dev/null)"; then
  echo "ERROR: failed to read tip from cardano-node (Haskell) at $HASKELL_SOCK" >&2
  exit 2
fi

if [[ -z "$haskell_tip_json" ]]; then
  echo "ERROR: failed to read tip from cardano-node (Haskell) at $HASKELL_SOCK" >&2
  exit 2
fi

if ! ygg_slot="$(extract_field yggdrasil slot 1 "$ygg_tip_json")"; then
  exit 2
fi
if ! ygg_hash="$(extract_field yggdrasil hash 1 "$ygg_tip_json")"; then
  exit 2
fi
if ! ygg_block="$(extract_field yggdrasil block 0 "$ygg_tip_json")"; then
  exit 2
fi
if ! ygg_epoch="$(extract_field yggdrasil epoch 0 "$ygg_tip_json")"; then
  exit 2
fi

if ! haskell_slot="$(extract_field haskell slot 1 "$haskell_tip_json")"; then
  exit 2
fi
if ! haskell_hash="$(extract_field haskell hash 1 "$haskell_tip_json")"; then
  exit 2
fi
if ! haskell_block="$(extract_field haskell block 0 "$haskell_tip_json")"; then
  exit 2
fi
if ! haskell_epoch="$(extract_field haskell epoch 0 "$haskell_tip_json")"; then
  exit 2
fi

now="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "[info] $now comparison results:"
printf '  yggdrasil: slot=%s block=%s epoch=%s hash=%s\n' \
  "$ygg_slot" "$ygg_block" "$ygg_epoch" "$ygg_hash"
printf '  haskell:   slot=%s block=%s epoch=%s hash=%s\n' \
  "$haskell_slot" "$haskell_block" "$haskell_epoch" "$haskell_hash"

# Match conditions: hash MUST match for "fully equal"; slot MUST match
# for "in sync". A slot match without a hash match would indicate a
# fork - surface as divergence.
if [[ "$ygg_slot" == "$haskell_slot" && "$ygg_hash" == "$haskell_hash" ]]; then
  echo "[ok] tips match"
  exit 0
fi

# Divergence - save snapshots, print diagnosis, exit 1.
ts="$(date -u +%Y%m%d-%H%M%S)"
snap_dir="$SNAPSHOT_DIR/$ts"
mkdir -p "$snap_dir"
printf '%s\n' "$ygg_tip_json" >"$snap_dir/yggdrasil-tip.json"
printf '%s\n' "$haskell_tip_json" >"$snap_dir/haskell-tip.json"

echo "[divergence] tips disagree" >&2
if [[ "$ygg_slot" != "$haskell_slot" ]]; then
  echo "  slot:  yggdrasil=$ygg_slot haskell=$haskell_slot" >&2
fi
if [[ "$ygg_hash" != "$haskell_hash" ]]; then
  echo "  hash:  yggdrasil=$ygg_hash haskell=$haskell_hash" >&2
fi
echo "[snapshot] $snap_dir" >&2
echo "  Decision tree:" >&2
echo "    1. If slot differs by >1, one node is behind - wait 30 s and rerun." >&2
echo "    2. If slot equal but hash differs at the same slot - likely a fork;" >&2
echo "       investigate whether yggdrasil saw an alt-chain. Rerun in 30 s." >&2
echo "    3. If divergence persists across 3 consecutive samples, this is a real" >&2
echo "       parity bug. Capture snapshot dirs and report to the parity audit." >&2
exit 1
