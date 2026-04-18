#!/usr/bin/env bash
set -euo pipefail

# Run yggdrasil-node as a preprod block producer using real pool credentials
# and validate key startup/forging-loop signals from logs.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOG_DIR="${LOG_DIR:-/tmp/ygg-real-preprod}"
DB_DIR="${DB_DIR:-/tmp/ygg-real-preprod-db}"
RUN_SECONDS="${RUN_SECONDS:-45}"
CARDANO_BIN_DIR="${CARDANO_BIN_DIR:-/tmp/cardano-bin}"
YGG_BIN="${YGG_BIN:-$ROOT_DIR/target/debug/yggdrasil-node}"
EXPECT_FORGE_EVENTS="${EXPECT_FORGE_EVENTS:-0}"
EXPECT_ADOPTED_EVENTS="${EXPECT_ADOPTED_EVENTS:-0}"

KESSKEY_PATH="${KES_SKEY_PATH:-}"
VRF_SKEY_PATH="${VRF_SKEY_PATH:-}"
OPCERT_PATH="${OPCERT_PATH:-}"
ISSUER_VKEY_PATH="${ISSUER_VKEY_PATH:-}"

usage() {
  cat <<'EOF'
Usage:
  KES_SKEY_PATH=/abs/path/kes.skey \
  VRF_SKEY_PATH=/abs/path/vrf.skey \
  OPCERT_PATH=/abs/path/node.cert \
  ISSUER_VKEY_PATH=/abs/path/cold.vkey \
  node/scripts/run_preprod_real_pool_producer.sh

Optional env:
  CARDANO_BIN_DIR   Default: /tmp/cardano-bin
  YGG_BIN           Default: target/debug/yggdrasil-node
  LOG_DIR           Default: /tmp/ygg-real-preprod
  DB_DIR            Default: /tmp/ygg-real-preprod-db
  RUN_SECONDS       Default: 45
  EXPECT_FORGE_EVENTS   Default: 0 (set 1 to require leader/forge evidence)
  EXPECT_ADOPTED_EVENTS Default: 0 (set 1 to require adopted forged block)

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
  if [[ ! -x "$CARDANO_BIN_DIR/cardano-cli" ]]; then
    echo "ERROR: cardano-cli not found at $CARDANO_BIN_DIR/cardano-cli" >&2
    return 1
  fi
  if [[ ! -x "$YGG_BIN" ]]; then
    echo "ERROR: yggdrasil-node binary not found at $YGG_BIN" >&2
    echo "Hint: run 'cargo build -p yggdrasil-node' first." >&2
    return 1
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

main() {
  if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
  fi

  ensure_tools
  require_file "$KESSKEY_PATH" "KES_SKEY_PATH"
  require_file "$VRF_SKEY_PATH" "VRF_SKEY_PATH"
  require_file "$OPCERT_PATH" "OPCERT_PATH"
  require_file "$ISSUER_VKEY_PATH" "ISSUER_VKEY_PATH"

  mkdir -p "$LOG_DIR" "$DB_DIR"
  local log_file="$LOG_DIR/preprod-real-pool-$(date +%Y%m%d-%H%M%S).log"

  echo "[info] cardano-cli version:"
  "$CARDANO_BIN_DIR/cardano-cli" --version | sed -n '1,2p'
  echo "[info] yggdrasil-node: $YGG_BIN"
  echo "[info] log file: $log_file"

  set +e
  (
    cd "$ROOT_DIR" &&
      "$YGG_BIN" run \
        --network preprod \
        --database-path "$DB_DIR" \
        --shelley-kes-key "$KESSKEY_PATH" \
        --shelley-vrf-key "$VRF_SKEY_PATH" \
        --shelley-operational-certificate "$OPCERT_PATH" \
        --shelley-operational-certificate-issuer-vkey "$ISSUER_VKEY_PATH"
  ) >"$log_file" 2>&1 &
  local pid=$!

  cleanup() {
    kill "$pid" >/dev/null 2>&1 || true
  }
  trap cleanup EXIT INT TERM

  local elapsed=0
  while [[ "$elapsed" -lt "$RUN_SECONDS" ]]; do
    if ! kill -0 "$pid" >/dev/null 2>&1; then
      echo "ERROR: yggdrasil-node exited before RUN_SECONDS elapsed" >&2
      echo "[info] last log lines:" >&2
      tail -n 60 "$log_file" >&2 || true
      exit 1
    fi
    sleep 1
    elapsed=$((elapsed + 1))
  done

  kill "$pid" >/dev/null 2>&1 || true
  wait "$pid" >/dev/null 2>&1 || true
  trap - EXIT INT TERM
  set -e

  echo "[info] verifying runtime signals..."

  if ! grep -q "Startup.BlockProducer" "$log_file"; then
    echo "ERROR: did not observe Startup.BlockProducer in logs" >&2
    exit 1
  fi
  if ! grep -q "block producer loop started" "$log_file"; then
    echo "ERROR: did not observe block producer loop start" >&2
    exit 1
  fi
  if grep -q "invalid VRF proof" "$log_file"; then
    echo "ERROR: observed invalid VRF proof in logs" >&2
    exit 1
  fi
  if ! grep -q "bootstrap peer connected" "$log_file"; then
    echo "ERROR: did not observe preprod bootstrap connection" >&2
    exit 1
  fi

  if [[ "$EXPECT_FORGE_EVENTS" == "1" ]]; then
    if ! grep -Eq "elected as slot leader|forged local block|adopted forged block|did not adopt forged block" "$log_file"; then
      echo "ERROR: EXPECT_FORGE_EVENTS=1 but no leader/forge evidence found" >&2
      echo "Hint: increase RUN_SECONDS (e.g., 600+) and confirm pool has active stake on preprod." >&2
      exit 1
    fi
  fi

  if [[ "$EXPECT_ADOPTED_EVENTS" == "1" ]]; then
    if ! grep -q "adopted forged block" "$log_file"; then
      echo "ERROR: EXPECT_ADOPTED_EVENTS=1 but no adopted forged block found" >&2
      echo "Hint: ensure the pool is active/registered and extend RUN_SECONDS." >&2
      exit 1
    fi
  fi

  summarize_evidence "$log_file"

  echo "[ok] producer-mode preprod verification checks passed"
  echo "[ok] log: $log_file"
}

main "$@"
