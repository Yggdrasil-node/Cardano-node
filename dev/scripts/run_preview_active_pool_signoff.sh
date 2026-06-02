#!/usr/bin/env bash
set -euo pipefail

# End-to-end preview producer sign-off wrapper for an already registered pool.
# It checks active-pool status first, ensures a queryable upstream Haskell
# preview socket, then delegates to run_preview_real_pool_producer.sh with the
# full 15m/60m/6h comparison window and forge/adoption gates enabled.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRIPT_DIR="$ROOT_DIR/scripts"
REF_RUN_NODE="$ROOT_DIR/.reference-haskell-cardano-node/install/run-node.sh"

CRED_DIR="${CRED_DIR:-}"
POOL_ID="${POOL_ID:-}"
NETWORK_MAGIC="${NETWORK_MAGIC:-2}"
RUN_SECONDS="${RUN_SECONDS:-21600}"
TIP_COMPARE_CHECKPOINTS="${TIP_COMPARE_CHECKPOINTS:-900,3600,21600}"
EXPECT_FORGE_EVENTS="${EXPECT_FORGE_EVENTS:-1}"
EXPECT_ADOPTED_EVENTS="${EXPECT_ADOPTED_EVENTS:-1}"
REQUIRE_TIP_COMPARISON="${REQUIRE_TIP_COMPARISON:-1}"
HASKELL_RUN_ROOT="${HASKELL_RUN_ROOT:-/tmp/ygg-haskell-preview}"
HASKELL_PORT="${HASKELL_PORT:-13003}"
HASKELL_START_TIMEOUT_S="${HASKELL_START_TIMEOUT_S:-180}"
HASKELL_LOG="${HASKELL_LOG:-/tmp/ygg-haskell-preview-signoff-$(date -u +%Y%m%dT%H%M%SZ).log}"
HASKELL_SOCK_EXPLICIT="${HASKELL_SOCK:-}"
HASKELL_SOCK="${HASKELL_SOCK:-$HASKELL_RUN_ROOT/preview/socket/node.socket}"
HASKELL_SYNC_MIN_PERCENT="${HASKELL_SYNC_MIN_PERCENT:-99.00}"
HASKELL_SYNC_TIMEOUT_S="${HASKELL_SYNC_TIMEOUT_S:-7200}"
HASKELL_SYNC_POLL_S="${HASKELL_SYNC_POLL_S:-30}"
KES_SKEY_PATH="${KES_SKEY_PATH:-${CRED_DIR:+$CRED_DIR/kes.skey}}"
VRF_SKEY_PATH="${VRF_SKEY_PATH:-${CRED_DIR:+$CRED_DIR/vrf.skey}}"
OPCERT_PATH="${OPCERT_PATH:-${CRED_DIR:+$CRED_DIR/node.cert}}"

if [[ -z "${CARDANO_CLI:-}" ]]; then
  if [[ -x "$ROOT_DIR/.reference-haskell-cardano-node/install/bin/cardano-cli" ]]; then
    CARDANO_CLI="$ROOT_DIR/.reference-haskell-cardano-node/install/bin/cardano-cli"
  else
    CARDANO_CLI="cardano-cli"
  fi
fi

usage() {
  cat <<'EOF'
Usage:
  CRED_DIR=/tmp/ygg-preview-generated-bp-... \
  POOL_ID=pool1... \
  dev/scripts/run_preview_active_pool_signoff.sh

Equivalent explicit credential form:
  POOL_ID=pool1... \
  KES_SKEY_PATH=/abs/path/kes.skey \
  VRF_SKEY_PATH=/abs/path/vrf.skey \
  OPCERT_PATH=/abs/path/node.cert \
  dev/scripts/run_preview_active_pool_signoff.sh

Default sign-off settings:
  RUN_SECONDS=21600
  TIP_COMPARE_CHECKPOINTS=900,3600,21600
  EXPECT_FORGE_EVENTS=1
  EXPECT_ADOPTED_EVENTS=1
  REQUIRE_TIP_COMPARISON=1

The wrapper first runs preview_pool_activation_status.sh with REQUIRE_ACTIVE=1.
If the pool is active, it uses HASKELL_SOCK when supplied and queryable;
otherwise it starts .reference-haskell-cardano-node/install/run-node.sh preview
under HASKELL_RUN_ROOT=/tmp/ygg-haskell-preview and waits for:
  cardano-cli query tip --testnet-magic 2
It then waits for the Haskell preview node to reach HASKELL_SYNC_MIN_PERCENT
before starting Yggdrasil, so the first 15-minute comparison checkpoint is not
spent catching up from Origin.

Optional env:
  CARDANO_CLI              Default: vendored 11.0.1 cardano-cli if present, else cardano-cli
  HASKELL_SOCK             Existing upstream preview socket to use instead of starting one
  HASKELL_RUN_ROOT         Default: /tmp/ygg-haskell-preview
  HASKELL_PORT             Default: 13003
  HASKELL_LOG              Default: /tmp/ygg-haskell-preview-signoff-<UTC>.log
  HASKELL_START_TIMEOUT_S  Default: 180
  HASKELL_SYNC_MIN_PERCENT Default: 99.00
  HASKELL_SYNC_TIMEOUT_S   Default: 7200
  HASKELL_SYNC_POLL_S      Default: 30

Exit codes:
  0   Sign-off runner completed and all enabled acceptance gates passed.
  1   Prerequisite, Haskell relay, or producer sign-off failure.
  2   Bad invocation.
  3   Pool is registered but not active yet.
EOF
}

require_file() {
  local path="$1"
  local name="$2"
  if [[ -z "$path" || ! -f "$path" ]]; then
    echo "ERROR: missing $name file: $path" >&2
    return 1
  fi
}

require_bool01() {
  local name="$1"
  local value="$2"
  if [[ "$value" != "0" && "$value" != "1" ]]; then
    echo "ERROR: $name must be 0 or 1, got '$value'" >&2
    return 2
  fi
}

require_positive_uint() {
  local name="$1"
  local value="$2"
  if [[ ! "$value" =~ ^[0-9]+$ || "$value" == "0" ]]; then
    echo "ERROR: $name must be a positive integer, got '$value'" >&2
    return 2
  fi
}

require_percent() {
  local name="$1"
  local value="$2"
  python3 - "$name" "$value" <<'PY'
import sys

name, value = sys.argv[1:3]
try:
    parsed = float(value)
except ValueError:
    print(f"ERROR: {name} must be a percentage between 0 and 100, got {value!r}", file=sys.stderr)
    sys.exit(2)

if not 0 <= parsed <= 100:
    print(f"ERROR: {name} must be a percentage between 0 and 100, got {value!r}", file=sys.stderr)
    sys.exit(2)
PY
}

query_haskell_tip() {
  [[ -S "$HASKELL_SOCK" ]] || return 1
  CARDANO_NODE_SOCKET_PATH="$HASKELL_SOCK" \
    "$CARDANO_CLI" query tip --testnet-magic 2 \
    >/dev/null 2>&1
}

query_haskell_tip_json() {
  [[ -S "$HASKELL_SOCK" ]] || return 1
  CARDANO_NODE_SOCKET_PATH="$HASKELL_SOCK" \
    "$CARDANO_CLI" query tip --testnet-magic 2
}

haskell_sync_progress_percent() {
  python3 -c '
import json
import re
import sys

try:
    tip = json.load(sys.stdin)
except Exception:
    sys.exit(1)

raw = str(tip.get("syncProgress", "")).strip()
match = re.search(r"[0-9]+(?:\.[0-9]+)?", raw)
if not match:
    sys.exit(1)
print(match.group(0))
'
}

wait_for_haskell_socket() {
  local pid="$1"
  local elapsed=0
  while (( elapsed < HASKELL_START_TIMEOUT_S )); do
    if ! kill -0 "$pid" >/dev/null 2>&1; then
      echo "ERROR: Haskell preview relay exited before socket readiness" >&2
      tail -n 80 "$HASKELL_LOG" >&2 || true
      return 1
    fi
    if query_haskell_tip; then
      echo "[ok] Haskell preview socket queryable: $HASKELL_SOCK"
      return 0
    fi
    sleep 1
    elapsed=$((elapsed + 1))
  done
  echo "ERROR: timed out waiting for Haskell preview socket: $HASKELL_SOCK" >&2
  tail -n 120 "$HASKELL_LOG" >&2 || true
  return 1
}

wait_for_haskell_sync() {
  local elapsed=0
  local tip_json progress ready
  while (( elapsed <= HASKELL_SYNC_TIMEOUT_S )); do
    if tip_json="$(query_haskell_tip_json 2>/dev/null)" &&
      progress="$(printf '%s\n' "$tip_json" | haskell_sync_progress_percent 2>/dev/null)"; then
      ready="$(python3 - "$progress" "$HASKELL_SYNC_MIN_PERCENT" <<'PY'
import sys

progress = float(sys.argv[1])
minimum = float(sys.argv[2])
print("1" if progress >= minimum else "0")
PY
)"
      echo "[info] Haskell preview syncProgress=${progress}% (required >= ${HASKELL_SYNC_MIN_PERCENT}%)"
      if [[ "$ready" == "1" ]]; then
        return 0
      fi
    else
      echo "[info] waiting for Haskell preview sync progress..."
    fi
    sleep "$HASKELL_SYNC_POLL_S"
    elapsed=$((elapsed + HASKELL_SYNC_POLL_S))
  done
  echo "ERROR: Haskell preview sync progress did not reach ${HASKELL_SYNC_MIN_PERCENT}% within ${HASKELL_SYNC_TIMEOUT_S}s" >&2
  return 1
}

main() {
  if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
  fi

  if [[ "$NETWORK_MAGIC" != "2" ]]; then
    echo "ERROR: run_preview_active_pool_signoff.sh is preview-only; NETWORK_MAGIC must be 2" >&2
    exit 2
  fi
  if [[ -z "$POOL_ID" ]]; then
    echo "ERROR: POOL_ID is required for active-pool sign-off" >&2
    usage >&2
    exit 2
  fi
  if ! command -v "$CARDANO_CLI" >/dev/null 2>&1; then
    echo "ERROR: cardano-cli not found (set CARDANO_CLI=/abs/path)" >&2
    exit 1
  fi
  require_file "$KES_SKEY_PATH" "KES_SKEY_PATH"
  require_file "$VRF_SKEY_PATH" "VRF_SKEY_PATH"
  require_file "$OPCERT_PATH" "OPCERT_PATH"
  require_positive_uint "RUN_SECONDS" "$RUN_SECONDS"
  require_positive_uint "HASKELL_PORT" "$HASKELL_PORT"
  require_positive_uint "HASKELL_START_TIMEOUT_S" "$HASKELL_START_TIMEOUT_S"
  require_positive_uint "HASKELL_SYNC_TIMEOUT_S" "$HASKELL_SYNC_TIMEOUT_S"
  require_positive_uint "HASKELL_SYNC_POLL_S" "$HASKELL_SYNC_POLL_S"
  require_percent "HASKELL_SYNC_MIN_PERCENT" "$HASKELL_SYNC_MIN_PERCENT"
  require_bool01 "EXPECT_FORGE_EVENTS" "$EXPECT_FORGE_EVENTS"
  require_bool01 "EXPECT_ADOPTED_EVENTS" "$EXPECT_ADOPTED_EVENTS"
  require_bool01 "REQUIRE_TIP_COMPARISON" "$REQUIRE_TIP_COMPARISON"

  echo "[info] checking active preview pool status..."
  CRED_DIR="$CRED_DIR" \
    POOL_ID="$POOL_ID" \
    REQUIRE_ACTIVE=1 \
    "$SCRIPT_DIR/preview_pool_activation_status.sh"

  local haskell_pid=""
  cleanup() {
    if [[ -n "$haskell_pid" ]]; then
      kill "$haskell_pid" >/dev/null 2>&1 || true
      wait "$haskell_pid" >/dev/null 2>&1 || true
    fi
  }
  trap cleanup EXIT INT TERM

  if query_haskell_tip; then
    echo "[ok] using existing queryable Haskell preview socket: $HASKELL_SOCK"
  else
    if [[ -n "$HASKELL_SOCK_EXPLICIT" ]]; then
      echo "ERROR: supplied HASKELL_SOCK is not a queryable preview socket: $HASKELL_SOCK" >&2
      exit 1
    fi
    if [[ ! -x "$REF_RUN_NODE" ]]; then
      echo "ERROR: reference Haskell launcher not executable: $REF_RUN_NODE" >&2
      exit 1
    fi
    rm -f "$HASKELL_SOCK"
    echo "[info] starting Haskell preview relay: RUN_ROOT=$HASKELL_RUN_ROOT PORT=$HASKELL_PORT log=$HASKELL_LOG"
    RUN_ROOT="$HASKELL_RUN_ROOT" PORT="$HASKELL_PORT" \
      "$REF_RUN_NODE" preview \
      >"$HASKELL_LOG" 2>&1 &
    haskell_pid=$!
    wait_for_haskell_socket "$haskell_pid"
  fi

  wait_for_haskell_sync

  echo "[info] starting preview active-pool producer sign-off..."
  KES_SKEY_PATH="$KES_SKEY_PATH" \
    VRF_SKEY_PATH="$VRF_SKEY_PATH" \
    OPCERT_PATH="$OPCERT_PATH" \
    HASKELL_SOCK="$HASKELL_SOCK" \
    CARDANO_CLI="$CARDANO_CLI" \
    RUN_SECONDS="$RUN_SECONDS" \
    TIP_COMPARE_CHECKPOINTS="$TIP_COMPARE_CHECKPOINTS" \
    EXPECT_FORGE_EVENTS="$EXPECT_FORGE_EVENTS" \
    EXPECT_ADOPTED_EVENTS="$EXPECT_ADOPTED_EVENTS" \
    REQUIRE_TIP_COMPARISON="$REQUIRE_TIP_COMPARISON" \
    "$SCRIPT_DIR/run_preview_real_pool_producer.sh"
}

main "$@"
