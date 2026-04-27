#!/usr/bin/env bash
set -euo pipefail

# Run yggdrasil-node as a mainnet block producer (or relay) using real pool
# credentials and validate key startup/forging-loop signals from logs.
#
# Mirrors run_preprod_real_pool_producer.sh; differences:
#   - --network mainnet, longer default RUN_SECONDS (600), and EXPECT_HOT_PEERS
#     defaults to 2 since mainnet should always reach >=2 hot peers within
#     the settle window.
#   - Credentials are MANDATORY for producer-mode runs; the script aborts
#     with usage if any of KES_SKEY_PATH / VRF_SKEY_PATH / OPCERT_PATH /
#     ISSUER_VKEY_PATH is unset. This is a safety guard: silently running
#     a producer without credentials on mainnet would create operational
#     ambiguity if it appears to start "fine".
#   - RELAY_ONLY=1 short-circuits the credential requirement and runs the
#     node in sync-only mode (recommended first step before producer mode).

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOG_DIR="${LOG_DIR:-/tmp/ygg-real-mainnet}"
DB_DIR="${DB_DIR:-/tmp/ygg-real-mainnet-db}"
RUN_SECONDS="${RUN_SECONDS:-600}"
CARDANO_BIN_DIR="${CARDANO_BIN_DIR:-/tmp/cardano-bin}"
YGG_BIN="${YGG_BIN:-$ROOT_DIR/target/debug/yggdrasil-node}"
EXPECT_FORGE_EVENTS="${EXPECT_FORGE_EVENTS:-0}"
EXPECT_ADOPTED_EVENTS="${EXPECT_ADOPTED_EVENTS:-0}"
EXPECT_HOT_PEERS="${EXPECT_HOT_PEERS:-2}"
RELAY_ONLY="${RELAY_ONLY:-0}"
METRICS_PORT="${METRICS_PORT:-9001}"

KESSKEY_PATH="${KES_SKEY_PATH:-}"
VRF_SKEY_PATH="${VRF_SKEY_PATH:-}"
OPCERT_PATH="${OPCERT_PATH:-}"
ISSUER_VKEY_PATH="${ISSUER_VKEY_PATH:-}"

usage() {
  cat <<'EOF'
Usage:

  Producer mode (requires real pool credentials on mainnet):
    KES_SKEY_PATH=/abs/path/kes.skey \
    VRF_SKEY_PATH=/abs/path/vrf.skey \
    OPCERT_PATH=/abs/path/node.cert \
    ISSUER_VKEY_PATH=/abs/path/cold.vkey \
    node/scripts/run_mainnet_real_pool_producer.sh

  Relay-only (sync without forging; recommended first dry run):
    RELAY_ONLY=1 node/scripts/run_mainnet_real_pool_producer.sh

Optional env:
  CARDANO_BIN_DIR        Default: /tmp/cardano-bin
  YGG_BIN                Default: target/debug/yggdrasil-node
  LOG_DIR                Default: /tmp/ygg-real-mainnet
  DB_DIR                 Default: /tmp/ygg-real-mainnet-db
  RUN_SECONDS            Default: 600 (10 min settle)
  METRICS_PORT           Default: 9001
  EXPECT_HOT_PEERS       Default: 2 (asserts >= N hot peers via Prometheus)
  EXPECT_FORGE_EVENTS    Default: 0 (set 1 to require leader/forge evidence)
  EXPECT_ADOPTED_EVENTS  Default: 0 (set 1 to require adopted forged block)

Exit codes:
  0   Verification checks passed.
  1   Missing prerequisites or verification failure.
EOF
}

require_file() {
  local p="$1"
  local name="$2"
  if [[ -z "$p" || ! -f "$p" ]]; then
    echo "ERROR: missing $name file: $p" >&2
    return 1
  fi
}

ensure_tools() {
  if [[ ! -x "$YGG_BIN" ]]; then
    echo "ERROR: yggdrasil-node binary not found at $YGG_BIN" >&2
    echo "Hint: run 'cargo build -p yggdrasil-node' first." >&2
    return 1
  fi
  # cardano-cli is optional for relay-only; required if we want hash-comparison.
  if [[ ! -x "$CARDANO_BIN_DIR/cardano-cli" ]]; then
    echo "[warn] cardano-cli not found at $CARDANO_BIN_DIR/cardano-cli (optional; needed for hash-comparison harness)" >&2
  fi
}

summarize_evidence() {
  local log_file="$1"
  local leader_count forged_count adopted_count not_adopted_count

  leader_count="$(grep -c "elected as slot leader" "$log_file" || true)"
  forged_count="$(grep -c "forged local block" "$log_file" || true)"
  adopted_count="$(grep -c "adopted forged block" "$log_file" || true)"
  not_adopted_count="$(grep -c "did not adopt forged block" "$log_file" || true)"

  echo "[info] evidence summary: leaders=$leader_count forged=$forged_count adopted=$adopted_count notAdopted=$not_adopted_count"
}

assert_hot_peers() {
  local expected="$1"
  if [[ "$expected" -le 0 ]]; then
    return 0
  fi
  local current
  current="$(curl -fsS "http://127.0.0.1:${METRICS_PORT}/metrics" 2>/dev/null \
    | grep -E '^yggdrasil_active_peers\s' \
    | awk '{print $2}' | head -1 || true)"
  if [[ -z "$current" ]]; then
    echo "[warn] could not read yggdrasil_active_peers from metrics endpoint" >&2
    return 0
  fi
  current="${current%.*}" # strip any decimal
  if [[ "$current" -lt "$expected" ]]; then
    echo "ERROR: yggdrasil_active_peers=$current < EXPECT_HOT_PEERS=$expected" >&2
    return 1
  fi
  echo "[ok] yggdrasil_active_peers=$current >= $expected"
}

main() {
  if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
  fi

  ensure_tools

  if [[ "$RELAY_ONLY" != "1" ]]; then
    require_file "$KESSKEY_PATH" "KES_SKEY_PATH"
    require_file "$VRF_SKEY_PATH" "VRF_SKEY_PATH"
    require_file "$OPCERT_PATH" "OPCERT_PATH"
    require_file "$ISSUER_VKEY_PATH" "ISSUER_VKEY_PATH"
  else
    echo "[info] RELAY_ONLY=1 — running in sync-only mode (no forging)"
  fi

  mkdir -p "$LOG_DIR" "$DB_DIR"
  local log_file="$LOG_DIR/mainnet-real-pool-$(date +%Y%m%d-%H%M%S).log"

  if [[ -x "$CARDANO_BIN_DIR/cardano-cli" ]]; then
    echo "[info] cardano-cli version:"
    "$CARDANO_BIN_DIR/cardano-cli" --version | sed -n '1,2p'
  fi
  echo "[info] yggdrasil-node: $YGG_BIN"
  echo "[info] log file:      $log_file"
  echo "[info] metrics port:  $METRICS_PORT"
  echo "[info] mode:          $([[ "$RELAY_ONLY" == "1" ]] && echo "relay-only" || echo "producer")"
  echo "[info] run window:    ${RUN_SECONDS}s"

  set +e
  local args=(
    run
    --network mainnet
    --database-path "$DB_DIR"
    --metrics-port "$METRICS_PORT"
  )
  if [[ "$RELAY_ONLY" != "1" ]]; then
    args+=(
      --shelley-kes-key "$KESSKEY_PATH"
      --shelley-vrf-key "$VRF_SKEY_PATH"
      --shelley-operational-certificate "$OPCERT_PATH"
      --shelley-operational-certificate-issuer-vkey "$ISSUER_VKEY_PATH"
    )
  fi

  ( cd "$ROOT_DIR" && "$YGG_BIN" "${args[@]}" ) >"$log_file" 2>&1 &
  local pid=$!

  cleanup() {
    kill "$pid" >/dev/null 2>&1 || true
  }
  trap cleanup EXIT INT TERM

  local elapsed=0
  local hot_peer_check_done=0
  while [[ "$elapsed" -lt "$RUN_SECONDS" ]]; do
    if ! kill -0 "$pid" >/dev/null 2>&1; then
      echo "ERROR: yggdrasil-node exited before RUN_SECONDS elapsed" >&2
      echo "[info] last log lines:" >&2
      tail -n 60 "$log_file" >&2 || true
      exit 1
    fi
    # One-shot hot-peer assertion at ~halfway point so the run still
    # observes the metrics surface even on short windows.
    if [[ "$hot_peer_check_done" -eq 0 && "$elapsed" -ge "$((RUN_SECONDS / 2))" ]]; then
      assert_hot_peers "$EXPECT_HOT_PEERS" || true
      hot_peer_check_done=1
    fi
    sleep 5
    elapsed=$((elapsed + 5))
  done

  # Final hot-peer check (authoritative).
  assert_hot_peers "$EXPECT_HOT_PEERS"
  local hot_peer_status=$?

  kill "$pid" >/dev/null 2>&1 || true
  wait "$pid" >/dev/null 2>&1 || true
  trap - EXIT INT TERM
  set -e

  echo "[info] verifying runtime signals..."

  # Mainnet relay path always reaches a bootstrap peer; producer mode also
  # logs the BlockProducer startup banner.
  if ! grep -q "bootstrap peer connected" "$log_file"; then
    echo "ERROR: did not observe mainnet bootstrap connection" >&2
    exit 1
  fi
  if grep -q "invalid VRF proof" "$log_file"; then
    echo "ERROR: observed invalid VRF proof in logs" >&2
    exit 1
  fi

  if [[ "$RELAY_ONLY" != "1" ]]; then
    if ! grep -q "Startup.BlockProducer" "$log_file"; then
      echo "ERROR: did not observe Startup.BlockProducer in logs" >&2
      exit 1
    fi
    if ! grep -q "block producer loop started" "$log_file"; then
      echo "ERROR: did not observe block producer loop start" >&2
      exit 1
    fi
  fi

  if [[ "$EXPECT_FORGE_EVENTS" == "1" ]]; then
    if ! grep -Eq "elected as slot leader|forged local block|adopted forged block|did not adopt forged block" "$log_file"; then
      echo "ERROR: EXPECT_FORGE_EVENTS=1 but no leader/forge evidence found" >&2
      echo "Hint: increase RUN_SECONDS (mainnet pool slots can be ~hours apart) and confirm pool has active stake." >&2
      exit 1
    fi
  fi

  if [[ "$EXPECT_ADOPTED_EVENTS" == "1" ]]; then
    if ! grep -q "adopted forged block" "$log_file"; then
      echo "ERROR: EXPECT_ADOPTED_EVENTS=1 but no adopted forged block found" >&2
      echo "Hint: ensure the pool is active/registered on mainnet and extend RUN_SECONDS." >&2
      exit 1
    fi
  fi

  summarize_evidence "$log_file"

  if [[ "$hot_peer_status" -ne 0 ]]; then
    exit 1
  fi

  echo "[ok] $([[ "$RELAY_ONLY" == "1" ]] && echo "relay-only" || echo "producer-mode") mainnet verification checks passed"
  echo "[ok] log: $log_file"
}

main "$@"
