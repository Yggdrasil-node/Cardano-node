#!/usr/bin/env bash
set -euo pipefail

# Parallel BlockFetch soak automation for the MANUAL_TEST_RUNBOOK.md section 6.5
# rehearsal. Starts yggdrasil-node with max_concurrent_block_fetch_peers > 1,
# captures Prometheus snapshots, verifies worker migration metrics, optionally
# compares tips against a Haskell cardano-node socket, and preserves logs.
#
# Usage:
#   YGG_BIN=target/release/yggdrasil-node \
#   NETWORK=preprod \
#   MAX_CONCURRENT_BLOCK_FETCH_PEERS=2 \
#   RUN_SECONDS=21600 \
#   HASKELL_SOCK=/tmp/cardano.sock \
#   REQUIRE_TIP_COMPARISON=1 \
#   scripts/parallel_blockfetch_soak.sh
#
# Exit codes:
#   0  soak completed and all enabled assertions passed
#   1  parity or liveness assertion failed
#   2  node startup, metrics, or invocation failure
#   3  --self-test failed

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ -z "${YGG_BIN:-}" ]]; then
  if [[ -x "$ROOT_DIR/target/release/yggdrasil-node" ]]; then
    YGG_BIN="$ROOT_DIR/target/release/yggdrasil-node"
  else
    YGG_BIN="$ROOT_DIR/target/debug/yggdrasil-node"
  fi
fi

NETWORK="${NETWORK:-preprod}"
CONFIG="${CONFIG:-}"
TOPOLOGY="${TOPOLOGY:-}"
MAX_CONCURRENT_BLOCK_FETCH_PEERS="${MAX_CONCURRENT_BLOCK_FETCH_PEERS:-2}"
EXPECT_WORKERS="${EXPECT_WORKERS:-$MAX_CONCURRENT_BLOCK_FETCH_PEERS}"
REQUIRE_WORKERS="${REQUIRE_WORKERS:-1}"
REQUIRE_PROGRESS="${REQUIRE_PROGRESS:-1}"
RUN_SECONDS="${RUN_SECONDS:-600}"
SAMPLE_INTERVAL_S="${SAMPLE_INTERVAL_S:-30}"
COMPARE_INTERVAL_S="${COMPARE_INTERVAL_S:-900}"
TIP_QUERY_TIMEOUT_SECONDS="${TIP_QUERY_TIMEOUT_SECONDS:-60}"
START_DEADLINE_S="${START_DEADLINE_S:-90}"
MIN_TIP_COMPARE_PASSES="${MIN_TIP_COMPARE_PASSES:-}"
CARDANO_CLI="${CARDANO_CLI:-cardano-cli}"
HASKELL_SOCK="${HASKELL_SOCK:-}"
REQUIRE_TIP_COMPARISON="${REQUIRE_TIP_COMPARISON:-0}"

case "$NETWORK" in
  mainnet) DEFAULT_NETWORK_MAGIC=764824073 ;;
  preprod) DEFAULT_NETWORK_MAGIC=1 ;;
  preview) DEFAULT_NETWORK_MAGIC=2 ;;
  *)
    echo "ERROR: NETWORK must be one of: mainnet, preprod, preview" >&2
    exit 2
    ;;
esac
NETWORK_MAGIC="${NETWORK_MAGIC:-$DEFAULT_NETWORK_MAGIC}"

RUN_DIR="${RUN_DIR:-$(mktemp -d -t ygg-blockfetch-soak-XXXXXX)}"
DB_DIR="${DB_DIR:-$RUN_DIR/db}"
SOCKET_PATH="${SOCKET_PATH:-$RUN_DIR/ygg.sock}"
LOG_DIR="${LOG_DIR:-$RUN_DIR/logs}"
METRICS_DIR="${METRICS_DIR:-$RUN_DIR/metrics}"

if [[ -z "${METRICS_PORT:-}" ]]; then
  METRICS_PORT="$(python3 -c \
    "import socket; s=socket.socket(); s.bind(('',0)); print(s.getsockname()[1])" \
    2>/dev/null || true)"
  if [[ -z "$METRICS_PORT" ]]; then
    METRICS_PORT=9201
  fi
fi

METRICS_URL="http://127.0.0.1:${METRICS_PORT}/metrics"

usage() {
  cat <<'EOF'
Usage:
  scripts/parallel_blockfetch_soak.sh --self-test

  or:

  NETWORK={mainnet|preprod|preview} \
  MAX_CONCURRENT_BLOCK_FETCH_PEERS=2 \
  RUN_SECONDS=21600 \
  REQUIRE_TIP_COMPARISON=1 \
  scripts/parallel_blockfetch_soak.sh

Optional env:
  YGG_BIN                           Default: target/release/yggdrasil-node if present, else target/debug/yggdrasil-node
  CONFIG                            Optional config file path; when set, --config is used instead of --network
  TOPOLOGY                          Optional P2P topology JSON path; passed as --topology
  NETWORK_MAGIC                     Default: derived from NETWORK (mainnet=764824073, preprod=1, preview=2)
  DB_DIR                            Default: $RUN_DIR/db
  SOCKET_PATH                       Default: $RUN_DIR/ygg.sock
  METRICS_PORT                      Default: free ephemeral port, or 9201 if python3 is unavailable
  LOG_DIR                           Default: $RUN_DIR/logs
  METRICS_DIR                       Default: $RUN_DIR/metrics
  RUN_SECONDS                       Default: 600; use 21600 for 6h, 86400 for 24h
  SAMPLE_INTERVAL_S                 Default: 30
  HASKELL_SOCK                      Optional cardano-node socket. Enables tip comparison.
  CARDANO_CLI                       Default: cardano-cli
  COMPARE_INTERVAL_S                Default: 900
  TIP_QUERY_TIMEOUT_SECONDS          Default: 60; per-node timeout inherited by compare_tip_to_haskell.sh
  MIN_TIP_COMPARE_PASSES            Default: floor(RUN_SECONDS / COMPARE_INTERVAL_S), minimum 2 in sign-off mode
  REQUIRE_TIP_COMPARISON            Default: 0. Set 1 for sign-off runs that require Haskell comparison evidence.
  EXPECT_WORKERS                    Default: MAX_CONCURRENT_BLOCK_FETCH_PEERS
                                    Must stay >= MAX_CONCURRENT_BLOCK_FETCH_PEERS for REQUIRE_TIP_COMPARISON=1 sign-off runs.
  REQUIRE_WORKERS                   Default: 1. Set 0 only for diagnostic captures; sign-off runs require 1.
  REQUIRE_PROGRESS                  Default: 1. Set 0 only when attaching to a deliberately idle network; sign-off runs require 1.

Required commands:
  curl, python3; cardano-cli when HASKELL_SOCK is set.

Exit codes:
  0  soak completed and all enabled assertions passed
  1  parity/liveness assertion failed
  2  startup, metrics, or invocation failure
  3  --self-test failed
EOF
}

if [[ "${1:-}" == "--self-test" ]]; then
  SELF_TEST=1
  shift
fi

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

is_uint() {
  [[ "$1" =~ ^[0-9]+$ ]]
}

require_uint() {
  local name="$1"
  local value="$2"
  if ! is_uint "$value"; then
    echo "ERROR: $name must be an unsigned integer, got '$value'" >&2
    exit 2
  fi
}

require_bool01() {
  local name="$1"
  local value="$2"
  if [[ "$value" != "0" && "$value" != "1" ]]; then
    echo "ERROR: $name must be 0 or 1, got '$value'" >&2
    exit 2
  fi
}

validate_signoff_contract() {
  if [[ "$REQUIRE_TIP_COMPARISON" != "1" ]]; then
    return 0
  fi

  local failed=0
  if [[ -z "$HASKELL_SOCK" ]]; then
    echo "ERROR: REQUIRE_TIP_COMPARISON=1 requires HASKELL_SOCK" >&2
    failed=2
  fi
  if (( EXPECT_WORKERS < MAX_CONCURRENT_BLOCK_FETCH_PEERS )); then
    echo "ERROR: REQUIRE_TIP_COMPARISON=1 requires EXPECT_WORKERS >= MAX_CONCURRENT_BLOCK_FETCH_PEERS" >&2
    failed=2
  fi
  if (( REQUIRE_WORKERS == 0 )); then
    echo "ERROR: REQUIRE_TIP_COMPARISON=1 requires REQUIRE_WORKERS=1 so worker activation cannot be bypassed" >&2
    failed=2
  fi
  if (( REQUIRE_PROGRESS == 0 )); then
    echo "ERROR: REQUIRE_TIP_COMPARISON=1 requires REQUIRE_PROGRESS=1 so idle tip equality cannot pass sign-off" >&2
    failed=2
  fi
  if (( COMPARE_INTERVAL_S > RUN_SECONDS )); then
    echo "ERROR: REQUIRE_TIP_COMPARISON=1 but COMPARE_INTERVAL_S=$COMPARE_INTERVAL_S exceeds RUN_SECONDS=$RUN_SECONDS" >&2
    failed=2
  fi
  if (( TIP_QUERY_TIMEOUT_SECONDS == 0 )); then
    echo "ERROR: REQUIRE_TIP_COMPARISON=1 requires TIP_QUERY_TIMEOUT_SECONDS > 0" >&2
    failed=2
  elif (( TIP_QUERY_TIMEOUT_SECONDS >= COMPARE_INTERVAL_S )); then
    echo "ERROR: REQUIRE_TIP_COMPARISON=1 requires TIP_QUERY_TIMEOUT_SECONDS < COMPARE_INTERVAL_S so a stale query cannot consume an entire comparison cadence" >&2
    failed=2
  fi
  if (( MIN_TIP_COMPARE_PASSES < 2 )); then
    echo "ERROR: REQUIRE_TIP_COMPARISON=1 requires MIN_TIP_COMPARE_PASSES >= 2" >&2
    failed=2
  fi
  if (( COMPARE_INTERVAL_S * MIN_TIP_COMPARE_PASSES > RUN_SECONDS )); then
    echo "ERROR: REQUIRE_TIP_COMPARISON=1 cannot fit MIN_TIP_COMPARE_PASSES=$MIN_TIP_COMPARE_PASSES at COMPARE_INTERVAL_S=$COMPARE_INTERVAL_S inside RUN_SECONDS=$RUN_SECONDS" >&2
    failed=2
  fi

  if (( failed != 0 )); then
    return "$failed"
  fi
  return 0
}

if [[ "${SELF_TEST:-0}" != "1" ]]; then
  require_uint "MAX_CONCURRENT_BLOCK_FETCH_PEERS" "$MAX_CONCURRENT_BLOCK_FETCH_PEERS"
  require_uint "EXPECT_WORKERS" "$EXPECT_WORKERS"
  require_bool01 "REQUIRE_WORKERS" "$REQUIRE_WORKERS"
  require_bool01 "REQUIRE_PROGRESS" "$REQUIRE_PROGRESS"
  require_uint "RUN_SECONDS" "$RUN_SECONDS"
  require_uint "SAMPLE_INTERVAL_S" "$SAMPLE_INTERVAL_S"
  require_uint "COMPARE_INTERVAL_S" "$COMPARE_INTERVAL_S"
  require_uint "TIP_QUERY_TIMEOUT_SECONDS" "$TIP_QUERY_TIMEOUT_SECONDS"
  require_uint "START_DEADLINE_S" "$START_DEADLINE_S"
  require_bool01 "REQUIRE_TIP_COMPARISON" "$REQUIRE_TIP_COMPARISON"

  if (( MAX_CONCURRENT_BLOCK_FETCH_PEERS < 2 )); then
    echo "ERROR: MAX_CONCURRENT_BLOCK_FETCH_PEERS must be >= 2 for parallel BlockFetch rehearsal" >&2
    exit 2
  fi

  if (( SAMPLE_INTERVAL_S == 0 )); then
    echo "ERROR: SAMPLE_INTERVAL_S must be > 0" >&2
    exit 2
  fi

  if (( COMPARE_INTERVAL_S == 0 )); then
    echo "ERROR: COMPARE_INTERVAL_S must be > 0" >&2
    exit 2
  fi

  if (( TIP_QUERY_TIMEOUT_SECONDS == 0 )); then
    echo "ERROR: TIP_QUERY_TIMEOUT_SECONDS must be > 0" >&2
    exit 2
  fi

  if [[ -z "$MIN_TIP_COMPARE_PASSES" ]]; then
    MIN_TIP_COMPARE_PASSES=$(( RUN_SECONDS / COMPARE_INTERVAL_S ))
    if (( MIN_TIP_COMPARE_PASSES < 2 )); then
      MIN_TIP_COMPARE_PASSES=2
    fi
  fi
  require_uint "MIN_TIP_COMPARE_PASSES" "$MIN_TIP_COMPARE_PASSES"

  if [[ ! -x "$YGG_BIN" ]]; then
    echo "ERROR: yggdrasil-node binary not found at $YGG_BIN" >&2
    exit 2
  fi

  if ! command -v curl >/dev/null 2>&1; then
    echo "ERROR: curl is required for Prometheus metrics sampling" >&2
    exit 2
  fi

  if ! command -v python3 >/dev/null 2>&1; then
    echo "ERROR: python3 is required for writing summary.json evidence" >&2
    exit 2
  fi

  if [[ -n "$CONFIG" && ! -f "$CONFIG" ]]; then
    echo "ERROR: CONFIG file not found: $CONFIG" >&2
    exit 2
  fi

  if [[ -n "$TOPOLOGY" && ! -f "$TOPOLOGY" ]]; then
    echo "ERROR: TOPOLOGY file not found: $TOPOLOGY" >&2
    exit 2
  fi

  if [[ -n "$HASKELL_SOCK" ]]; then
    if ! command -v "$CARDANO_CLI" >/dev/null 2>&1; then
      echo "ERROR: CARDANO_CLI not found in PATH (set CARDANO_CLI=/abs/path)" >&2
      exit 2
    fi
    if [[ ! -S "$HASKELL_SOCK" ]]; then
      echo "ERROR: HASKELL_SOCK is not a unix socket: $HASKELL_SOCK" >&2
      exit 2
    fi
  fi

  validate_signoff_contract || exit $?
fi

mkdir -p "$DB_DIR" "$LOG_DIR" "$METRICS_DIR"
rm -f "$SOCKET_PATH"

node_log="$LOG_DIR/yggdrasil-node.log"
summary_file="$LOG_DIR/summary.txt"
summary_json="$LOG_DIR/summary.json"
pid=""

stop_node() {
  local node_pid="${1:-}"
  if [[ -z "$node_pid" ]]; then
    return 0
  fi
  if ! kill -0 "$node_pid" >/dev/null 2>&1; then
    wait "$node_pid" >/dev/null 2>&1 || true
    return 0
  fi
  kill -TERM "$node_pid" >/dev/null 2>&1 || true
  for _ in 1 2 3 4 5; do
    if ! kill -0 "$node_pid" >/dev/null 2>&1; then
      wait "$node_pid" >/dev/null 2>&1 || true
      return 0
    fi
    sleep 1
  done
  kill -9 "$node_pid" >/dev/null 2>&1 || true
  wait "$node_pid" >/dev/null 2>&1 || true
}

trap 'stop_node "$pid"' EXIT INT TERM

start_node() {
  local args=(run)
  if [[ -n "$CONFIG" ]]; then
    args+=(--config "$CONFIG")
  else
    args+=(--network "$NETWORK")
  fi
  if [[ -n "$TOPOLOGY" ]]; then
    args+=(--topology "$TOPOLOGY")
  fi
  args+=(
    --database-path "$DB_DIR"
    --socket-path "$SOCKET_PATH"
    --metrics-port "$METRICS_PORT"
    --max-concurrent-block-fetch-peers "$MAX_CONCURRENT_BLOCK_FETCH_PEERS"
  )

  (cd "$ROOT_DIR" && "$YGG_BIN" "${args[@]}") >"$node_log" 2>&1 &
  echo $!
}

wait_for_metrics() {
  local node_pid="$1"
  local deadline=$(( $(date +%s) + START_DEADLINE_S ))
  while [[ "$(date +%s)" -lt "$deadline" ]]; do
    if ! kill -0 "$node_pid" >/dev/null 2>&1; then
      return 1
    fi
    if curl -fsS "$METRICS_URL" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  return 1
}

sample_metrics() {
  local label="$1"
  local ts
  ts="$(date -u +%Y%m%dT%H%M%SZ)"
  local file="$METRICS_DIR/${label}-${ts}.prom"
  curl -fsS "$METRICS_URL" >"$file"
  echo "$file"
}

metric_value() {
  local metric="$1"
  local file="$2"
  awk -v name="$metric" '$1 == name { print $2; found=1; exit } END { if (!found) print "" }' "$file"
}

metric_or_zero() {
  local metric="$1"
  local file="$2"
  local value
  value="$(metric_value "$metric" "$file")"
  if [[ -z "$value" ]]; then
    echo 0
  else
    echo "$value"
  fi
}

numeric_gt() {
  awk -v a="${1:-0}" -v b="${2:-0}" 'BEGIN { exit !((a + 0) > (b + 0)) }'
}

numeric_ge() {
  awk -v a="${1:-0}" -v b="${2:-0}" 'BEGIN { exit !((a + 0) >= (b + 0)) }'
}

avg_metric() {
  local sum="$1"
  local count="$2"
  awk -v sum="${sum:-0}" -v count="${count:-0}" 'BEGIN {
    if ((count + 0) == 0) {
      print "n/a";
    } else {
      printf "%.3fs", (sum + 0) / (count + 0);
    }
  }'
}

write_summary_json() {
  local output="$1"
  SUMMARY_NETWORK="$NETWORK" \
  SUMMARY_NETWORK_MAGIC="$NETWORK_MAGIC" \
  SUMMARY_MAX_CONCURRENT_BLOCK_FETCH_PEERS="$MAX_CONCURRENT_BLOCK_FETCH_PEERS" \
  SUMMARY_EXPECT_WORKERS="$EXPECT_WORKERS" \
  SUMMARY_REQUIRE_WORKERS="$REQUIRE_WORKERS" \
  SUMMARY_REQUIRE_PROGRESS="$REQUIRE_PROGRESS" \
  SUMMARY_REQUIRE_TIP_COMPARISON="$REQUIRE_TIP_COMPARISON" \
  SUMMARY_RUN_SECONDS="$RUN_SECONDS" \
  SUMMARY_SAMPLE_INTERVAL_S="$SAMPLE_INTERVAL_S" \
  SUMMARY_COMPARE_INTERVAL_S="$COMPARE_INTERVAL_S" \
  SUMMARY_TIP_QUERY_TIMEOUT_SECONDS="$TIP_QUERY_TIMEOUT_SECONDS" \
  SUMMARY_MIN_TIP_COMPARE_PASSES="$MIN_TIP_COMPARE_PASSES" \
  SUMMARY_START_BLOCKS="$start_blocks" \
  SUMMARY_END_BLOCKS="$end_blocks" \
  SUMMARY_START_SLOT="$start_slot" \
  SUMMARY_END_SLOT="$end_slot" \
  SUMMARY_START_RECONNECTS="$start_reconnects" \
  SUMMARY_END_RECONNECTS="$end_reconnects" \
  SUMMARY_MAX_WORKERS="$max_workers" \
  SUMMARY_END_WORKERS="$end_workers" \
  SUMMARY_MIGRATED_TOTAL="$migrated_total" \
  SUMMARY_WORKER_SHORTFALL_SAMPLES="$worker_shortfall_samples" \
  SUMMARY_FETCH_SUM="$fetch_sum" \
  SUMMARY_FETCH_COUNT="$fetch_count" \
  SUMMARY_FETCH_AVG="$fetch_avg" \
  SUMMARY_APPLY_SUM="$apply_sum" \
  SUMMARY_APPLY_COUNT="$apply_count" \
  SUMMARY_APPLY_AVG="$apply_avg" \
  SUMMARY_TIP_COMPARE_PASSES="$compare_passes" \
  SUMMARY_RUN_DIR="$RUN_DIR" \
  SUMMARY_DB_DIR="$DB_DIR" \
  SUMMARY_SOCKET_PATH="$SOCKET_PATH" \
  SUMMARY_NODE_LOG="$node_log" \
  SUMMARY_METRICS_DIR="$METRICS_DIR" \
  SUMMARY_LOG_DIR="$LOG_DIR" \
  SUMMARY_TEXT="$summary_file" \
  python3 - "$output" <<'PY'
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path


def value(name: str, default: str = "") -> str:
    return os.environ.get(name, default)


def number(name: str, default: str = "0"):
    raw = value(name, default)
    if raw == "":
        return 0
    try:
        if "." in raw:
            return float(raw)
        return int(raw)
    except ValueError:
        return raw


def bool01(name: str) -> bool:
    return value(name, "0") == "1"


log_dir = Path(value("SUMMARY_LOG_DIR"))
tip_compare_logs = sorted(str(path) for path in log_dir.glob("tip-compare-*.log"))

payload = {
    "schema_version": 1,
    "blocker": "blockfetch-section-6.5",
    "status": "pass",
    "generated_at_utc": datetime.now(timezone.utc)
    .replace(microsecond=0)
    .isoformat()
    .replace("+00:00", "Z"),
    "network": value("SUMMARY_NETWORK"),
    "network_magic": number("SUMMARY_NETWORK_MAGIC"),
    "run": {
        "run_seconds": number("SUMMARY_RUN_SECONDS"),
        "sample_interval_seconds": number("SUMMARY_SAMPLE_INTERVAL_S"),
        "compare_interval_seconds": number("SUMMARY_COMPARE_INTERVAL_S"),
        "tip_query_timeout_seconds": number("SUMMARY_TIP_QUERY_TIMEOUT_SECONDS"),
    },
    "worker_assertions": {
        "max_concurrent_block_fetch_peers": number(
            "SUMMARY_MAX_CONCURRENT_BLOCK_FETCH_PEERS"
        ),
        "expected_workers": number("SUMMARY_EXPECT_WORKERS"),
        "require_workers": bool01("SUMMARY_REQUIRE_WORKERS"),
        "workers_registered_max": number("SUMMARY_MAX_WORKERS"),
        "workers_registered_final": number("SUMMARY_END_WORKERS"),
        "workers_migrated_total": number("SUMMARY_MIGRATED_TOTAL"),
        "worker_shortfall_samples": number("SUMMARY_WORKER_SHORTFALL_SAMPLES"),
    },
    "progress_assertions": {
        "require_progress": bool01("SUMMARY_REQUIRE_PROGRESS"),
        "blocks_synced": {
            "start": number("SUMMARY_START_BLOCKS"),
            "end": number("SUMMARY_END_BLOCKS"),
        },
        "current_slot": {
            "start": number("SUMMARY_START_SLOT"),
            "end": number("SUMMARY_END_SLOT"),
        },
        "reconnects": {
            "start": number("SUMMARY_START_RECONNECTS"),
            "end": number("SUMMARY_END_RECONNECTS"),
        },
    },
    "performance": {
        "fetch_batch_duration_seconds_sum": number("SUMMARY_FETCH_SUM"),
        "fetch_batch_duration_seconds_count": number("SUMMARY_FETCH_COUNT"),
        "fetch_avg_per_batch": value("SUMMARY_FETCH_AVG"),
        "apply_batch_duration_seconds_sum": number("SUMMARY_APPLY_SUM"),
        "apply_batch_duration_seconds_count": number("SUMMARY_APPLY_COUNT"),
        "apply_avg_per_batch": value("SUMMARY_APPLY_AVG"),
    },
    "tip_comparison": {
        "require_tip_comparison": bool01("SUMMARY_REQUIRE_TIP_COMPARISON"),
        "min_tip_compare_passes": number("SUMMARY_MIN_TIP_COMPARE_PASSES"),
        "tip_compare_passes": number("SUMMARY_TIP_COMPARE_PASSES"),
        "tip_compare_log_count": len(tip_compare_logs),
        "tip_compare_logs": tip_compare_logs,
    },
    "artifacts": {
        "run_dir": value("SUMMARY_RUN_DIR"),
        "db_dir": value("SUMMARY_DB_DIR"),
        "socket_path": value("SUMMARY_SOCKET_PATH"),
        "log_dir": value("SUMMARY_LOG_DIR"),
        "node_log": value("SUMMARY_NODE_LOG"),
        "metrics_dir": value("SUMMARY_METRICS_DIR"),
        "summary_txt": value("SUMMARY_TEXT"),
        "tip_snapshots_dir": str(log_dir / "tip-snapshots"),
    },
}

Path(sys.argv[1]).write_text(
    json.dumps(payload, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
PY
}

run_self_test() {
  local self_dir="$ROOT_DIR/target/blockfetch-soak-self-test"
  local metrics_file="$self_dir/sample.prom"
  mkdir -p "$self_dir"
  cat >"$metrics_file" <<'EOF'
yggdrasil_blocks_synced 10
yggdrasil_current_slot 429460
yggdrasil_blockfetch_workers_registered 2
yggdrasil_blockfetch_workers_migrated_total 2
yggdrasil_fetch_batch_duration_seconds_sum 9
yggdrasil_fetch_batch_duration_seconds_count 3
EOF

  if [[ "$(metric_value yggdrasil_blockfetch_workers_registered "$metrics_file")" != "2" ]]; then
    echo "ERROR: metric_value failed worker lookup" >&2
    return 3
  fi
  if [[ "$(metric_or_zero missing_metric "$metrics_file")" != "0" ]]; then
    echo "ERROR: metric_or_zero failed missing-metric fallback" >&2
    return 3
  fi
  if ! numeric_gt 3 2 || numeric_gt 2 3; then
    echo "ERROR: numeric_gt comparison failed" >&2
    return 3
  fi
  if ! numeric_ge 2 2 || numeric_ge 1 2; then
    echo "ERROR: numeric_ge comparison failed" >&2
    return 3
  fi
  if [[ "$(avg_metric 9 3)" != "3.000s" ]]; then
    echo "ERROR: avg_metric calculation failed" >&2
    return 3
  fi
  if (
    REQUIRE_TIP_COMPARISON=1
    MAX_CONCURRENT_BLOCK_FETCH_PEERS=2
    HASKELL_SOCK=
    EXPECT_WORKERS=2
    REQUIRE_WORKERS=1
    REQUIRE_PROGRESS=1
    RUN_SECONDS=600
    COMPARE_INTERVAL_S=60
    TIP_QUERY_TIMEOUT_SECONDS=30
    MIN_TIP_COMPARE_PASSES=2
    validate_signoff_contract >/dev/null 2>&1
  ); then
    echo "ERROR: sign-off contract accepted missing HASKELL_SOCK" >&2
    return 3
  fi
  if (
    REQUIRE_TIP_COMPARISON=1
    MAX_CONCURRENT_BLOCK_FETCH_PEERS=2
    HASKELL_SOCK=/tmp/cardano.sock
    EXPECT_WORKERS=1
    REQUIRE_WORKERS=1
    REQUIRE_PROGRESS=1
    RUN_SECONDS=600
    COMPARE_INTERVAL_S=60
    TIP_QUERY_TIMEOUT_SECONDS=30
    MIN_TIP_COMPARE_PASSES=2
    validate_signoff_contract >/dev/null 2>&1
  ); then
    echo "ERROR: sign-off contract accepted EXPECT_WORKERS below the configured knob" >&2
    return 3
  fi
  if (
    REQUIRE_TIP_COMPARISON=1
    MAX_CONCURRENT_BLOCK_FETCH_PEERS=2
    HASKELL_SOCK=/tmp/cardano.sock
    EXPECT_WORKERS=2
    REQUIRE_WORKERS=0
    REQUIRE_PROGRESS=1
    RUN_SECONDS=600
    COMPARE_INTERVAL_S=60
    TIP_QUERY_TIMEOUT_SECONDS=30
    MIN_TIP_COMPARE_PASSES=2
    validate_signoff_contract >/dev/null 2>&1
  ); then
    echo "ERROR: sign-off contract accepted REQUIRE_WORKERS=0" >&2
    return 3
  fi
  if (
    REQUIRE_TIP_COMPARISON=1
    MAX_CONCURRENT_BLOCK_FETCH_PEERS=2
    HASKELL_SOCK=/tmp/cardano.sock
    EXPECT_WORKERS=2
    REQUIRE_WORKERS=1
    REQUIRE_PROGRESS=0
    RUN_SECONDS=600
    COMPARE_INTERVAL_S=60
    TIP_QUERY_TIMEOUT_SECONDS=30
    MIN_TIP_COMPARE_PASSES=2
    validate_signoff_contract >/dev/null 2>&1
  ); then
    echo "ERROR: sign-off contract accepted REQUIRE_PROGRESS=0" >&2
    return 3
  fi
  if (
    REQUIRE_TIP_COMPARISON=1
    MAX_CONCURRENT_BLOCK_FETCH_PEERS=2
    HASKELL_SOCK=/tmp/cardano.sock
    EXPECT_WORKERS=2
    REQUIRE_WORKERS=1
    REQUIRE_PROGRESS=1
    RUN_SECONDS=600
    COMPARE_INTERVAL_S=60
    TIP_QUERY_TIMEOUT_SECONDS=30
    MIN_TIP_COMPARE_PASSES=1
    validate_signoff_contract >/dev/null 2>&1
  ); then
    echo "ERROR: sign-off contract accepted MIN_TIP_COMPARE_PASSES < 2" >&2
    return 3
  fi
  if (
    REQUIRE_TIP_COMPARISON=1
    MAX_CONCURRENT_BLOCK_FETCH_PEERS=2
    HASKELL_SOCK=/tmp/cardano.sock
    EXPECT_WORKERS=2
    REQUIRE_WORKERS=1
    REQUIRE_PROGRESS=1
    RUN_SECONDS=600
    COMPARE_INTERVAL_S=400
    TIP_QUERY_TIMEOUT_SECONDS=30
    MIN_TIP_COMPARE_PASSES=2
    validate_signoff_contract >/dev/null 2>&1
  ); then
    echo "ERROR: sign-off contract accepted too few comparison slots in the run window" >&2
    return 3
  fi
  if (
    REQUIRE_TIP_COMPARISON=1
    MAX_CONCURRENT_BLOCK_FETCH_PEERS=2
    HASKELL_SOCK=/tmp/cardano.sock
    EXPECT_WORKERS=2
    REQUIRE_WORKERS=1
    REQUIRE_PROGRESS=1
    RUN_SECONDS=600
    COMPARE_INTERVAL_S=60
    TIP_QUERY_TIMEOUT_SECONDS=60
    MIN_TIP_COMPARE_PASSES=2
    validate_signoff_contract >/dev/null 2>&1
  ); then
    echo "ERROR: sign-off contract accepted timeout equal to the comparison cadence" >&2
    return 3
  fi
  if ! (
    REQUIRE_TIP_COMPARISON=1
    MAX_CONCURRENT_BLOCK_FETCH_PEERS=2
    HASKELL_SOCK=/tmp/cardano.sock
    EXPECT_WORKERS=2
    REQUIRE_WORKERS=1
    REQUIRE_PROGRESS=1
    RUN_SECONDS=600
    COMPARE_INTERVAL_S=60
    TIP_QUERY_TIMEOUT_SECONDS=30
    MIN_TIP_COMPARE_PASSES=2
    validate_signoff_contract >/dev/null 2>&1
  ); then
    echo "ERROR: sign-off contract rejected a valid strict configuration" >&2
    return 3
  fi

  local summary_json_file="$self_dir/summary.json"
  local summary_file="$self_dir/summary.txt"
  local node_log="$self_dir/yggdrasil-node.log"
  local LOG_DIR="$self_dir/logs"
  local METRICS_DIR="$self_dir/metrics"
  local RUN_DIR="$self_dir"
  local DB_DIR="$self_dir/db"
  local SOCKET_PATH="$self_dir/ygg.sock"
  local NETWORK=preprod
  local NETWORK_MAGIC=1
  local MAX_CONCURRENT_BLOCK_FETCH_PEERS=2
  local EXPECT_WORKERS=2
  local REQUIRE_WORKERS=1
  local REQUIRE_PROGRESS=1
  local REQUIRE_TIP_COMPARISON=1
  local RUN_SECONDS=600
  local SAMPLE_INTERVAL_S=30
  local COMPARE_INTERVAL_S=60
  local TIP_QUERY_TIMEOUT_SECONDS=30
  local MIN_TIP_COMPARE_PASSES=2
  local start_blocks=10
  local end_blocks=42
  local start_slot=429460
  local end_slot=429900
  local start_reconnects=0
  local end_reconnects=0
  local max_workers=2
  local end_workers=2
  local migrated_total=2
  local worker_shortfall_samples=0
  local fetch_sum=9
  local fetch_count=3
  local fetch_avg="3.000s"
  local apply_sum=6
  local apply_count=3
  local apply_avg="2.000s"
  local compare_passes=2
  mkdir -p "$LOG_DIR" "$METRICS_DIR"
  printf 'tip comparison 1\n' >"$LOG_DIR/tip-compare-20260501T000000Z.log"
  printf 'tip comparison 2\n' >"$LOG_DIR/tip-compare-20260501T000100Z.log"
  write_summary_json "$summary_json_file"
  python3 - "$summary_json_file" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    summary = json.load(handle)

assert summary["schema_version"] == 1
assert summary["blocker"] == "blockfetch-section-6.5"
assert summary["status"] == "pass"
assert summary["network"] == "preprod"
assert summary["worker_assertions"]["expected_workers"] == 2
assert summary["worker_assertions"]["worker_shortfall_samples"] == 0
assert summary["progress_assertions"]["current_slot"]["end"] == 429900
assert summary["tip_comparison"]["require_tip_comparison"] is True
assert summary["tip_comparison"]["tip_compare_passes"] == 2
assert summary["tip_comparison"]["tip_compare_log_count"] == 2
assert len(summary["tip_comparison"]["tip_compare_logs"]) == 2
assert summary["run"]["tip_query_timeout_seconds"] == 30
assert summary["artifacts"]["summary_txt"].endswith("summary.txt")
assert summary["artifacts"]["tip_snapshots_dir"].endswith("tip-snapshots")
PY

  echo "[ok] parallel_blockfetch_soak self-test passed"
}

if [[ "${SELF_TEST:-0}" == "1" ]]; then
  run_self_test
  exit $?
fi

run_tip_compare() {
  local ts
  ts="$(date -u +%Y%m%dT%H%M%SZ)"
  local logfile="$LOG_DIR/tip-compare-${ts}.log"
  SNAPSHOT_DIR="$LOG_DIR/tip-snapshots" \
    YGG_BIN="$YGG_BIN" \
    CARDANO_CLI="$CARDANO_CLI" \
    YGG_SOCK="$SOCKET_PATH" \
    HASKELL_SOCK="$HASKELL_SOCK" \
    NETWORK_MAGIC="$NETWORK_MAGIC" \
    TIP_QUERY_TIMEOUT_SECONDS="$TIP_QUERY_TIMEOUT_SECONDS" \
    "$(dirname "${BASH_SOURCE[0]}")/compare_tip_to_haskell.sh" >"$logfile" 2>&1
}

echo "[info] parallel_blockfetch_soak: NETWORK=$NETWORK magic=$NETWORK_MAGIC knob=$MAX_CONCURRENT_BLOCK_FETCH_PEERS expected_workers=$EXPECT_WORKERS"
echo "[info] RUN_SECONDS=$RUN_SECONDS SAMPLE_INTERVAL_S=$SAMPLE_INTERVAL_S COMPARE_INTERVAL_S=$COMPARE_INTERVAL_S TIP_QUERY_TIMEOUT_SECONDS=$TIP_QUERY_TIMEOUT_SECONDS MIN_TIP_COMPARE_PASSES=$MIN_TIP_COMPARE_PASSES"
echo "[info] DB_DIR=$DB_DIR SOCKET_PATH=$SOCKET_PATH METRICS_URL=$METRICS_URL"
echo "[info] LOG_DIR=$LOG_DIR METRICS_DIR=$METRICS_DIR"
if [[ -n "$HASKELL_SOCK" ]]; then
  echo "[info] Haskell tip comparison enabled: HASKELL_SOCK=$HASKELL_SOCK"
else
  echo "[info] Haskell tip comparison disabled: set HASKELL_SOCK to enable"
fi

pid="$(start_node)"
if ! wait_for_metrics "$pid"; then
  echo "ERROR: yggdrasil-node did not expose /metrics within ${START_DEADLINE_S}s" >&2
  echo "[forensic] node log: $node_log" >&2
  exit 2
fi

start_file="$(sample_metrics start)"
start_blocks="$(metric_or_zero yggdrasil_blocks_synced "$start_file")"
start_slot="$(metric_or_zero yggdrasil_current_slot "$start_file")"
start_reconnects="$(metric_or_zero yggdrasil_reconnects "$start_file")"
max_workers="$(metric_or_zero yggdrasil_blockfetch_workers_registered "$start_file")"
last_file="$start_file"
compare_passes=0
workers_activated=0
worker_shortfall_samples=0
if numeric_ge "$max_workers" "$EXPECT_WORKERS"; then
  workers_activated=1
fi

start_epoch="$(date +%s)"
end_epoch=$(( start_epoch + RUN_SECONDS ))
next_compare_epoch=$(( start_epoch + COMPARE_INTERVAL_S ))

while [[ "$(date +%s)" -lt "$end_epoch" ]]; do
  if ! kill -0 "$pid" >/dev/null 2>&1; then
    echo "ERROR: yggdrasil-node exited before soak completed" >&2
    echo "[forensic] node log: $node_log" >&2
    exit 2
  fi

  now_epoch="$(date +%s)"
  remaining=$(( end_epoch - now_epoch ))
  sleep_for="$SAMPLE_INTERVAL_S"
  if (( remaining < sleep_for )); then
    sleep_for="$remaining"
  fi
  if (( sleep_for > 0 )); then
    sleep "$sleep_for"
  fi

  last_file="$(sample_metrics sample)"
  workers="$(metric_or_zero yggdrasil_blockfetch_workers_registered "$last_file")"
  if numeric_gt "$workers" "$max_workers"; then
    max_workers="$workers"
  fi
  if numeric_ge "$workers" "$EXPECT_WORKERS"; then
    workers_activated=1
  elif (( workers_activated != 0 )); then
    worker_shortfall_samples=$(( worker_shortfall_samples + 1 ))
  fi

  now_epoch="$(date +%s)"
  if [[ -n "$HASKELL_SOCK" && "$now_epoch" -ge "$next_compare_epoch" ]]; then
    if run_tip_compare; then
      compare_passes=$(( compare_passes + 1 ))
    else
      echo "ERROR: tip comparison against Haskell node failed" >&2
      echo "[forensic] latest compare log under: $LOG_DIR" >&2
      exit 1
    fi
    next_compare_epoch=$(( now_epoch + COMPARE_INTERVAL_S ))
  fi
done

end_file="$(sample_metrics final)"
end_blocks="$(metric_or_zero yggdrasil_blocks_synced "$end_file")"
end_slot="$(metric_or_zero yggdrasil_current_slot "$end_file")"
end_reconnects="$(metric_or_zero yggdrasil_reconnects "$end_file")"
end_workers="$(metric_or_zero yggdrasil_blockfetch_workers_registered "$end_file")"
migrated_total="$(metric_or_zero yggdrasil_blockfetch_workers_migrated_total "$end_file")"
fetch_sum="$(metric_or_zero yggdrasil_fetch_batch_duration_seconds_sum "$end_file")"
fetch_count="$(metric_or_zero yggdrasil_fetch_batch_duration_seconds_count "$end_file")"
apply_sum="$(metric_or_zero yggdrasil_apply_batch_duration_seconds_sum "$end_file")"
apply_count="$(metric_or_zero yggdrasil_apply_batch_duration_seconds_count "$end_file")"

if numeric_gt "$end_workers" "$max_workers"; then
  max_workers="$end_workers"
fi
if numeric_ge "$end_workers" "$EXPECT_WORKERS"; then
  workers_activated=1
elif (( workers_activated != 0 )); then
  worker_shortfall_samples=$(( worker_shortfall_samples + 1 ))
fi

stop_node "$pid"
pid=""

if grep -E 'fetch worker channel closed|fetch worker dropped response' "$node_log" >/dev/null 2>&1; then
  echo "ERROR: BlockFetch worker failure trace detected" >&2
  echo "[forensic] node log: $node_log" >&2
  exit 1
fi

if (( REQUIRE_WORKERS != 0 )); then
  if ! numeric_ge "$max_workers" "$EXPECT_WORKERS"; then
    echo "ERROR: worker pool never reached EXPECT_WORKERS=$EXPECT_WORKERS (max observed $max_workers)" >&2
    echo "[forensic] metrics snapshots: $METRICS_DIR" >&2
    exit 1
  fi
  if ! numeric_ge "$migrated_total" "$EXPECT_WORKERS"; then
    echo "ERROR: migrated worker counter $migrated_total < EXPECT_WORKERS=$EXPECT_WORKERS" >&2
    echo "[forensic] node log: $node_log" >&2
    exit 1
  fi
fi

if (( REQUIRE_PROGRESS != 0 )); then
  if ! numeric_gt "$end_blocks" "$start_blocks" && ! numeric_gt "$end_slot" "$start_slot"; then
    echo "ERROR: no sync progress observed: blocks $start_blocks -> $end_blocks, slot $start_slot -> $end_slot" >&2
    echo "[forensic] metrics snapshots: $METRICS_DIR" >&2
    exit 1
  fi
fi

if [[ "$REQUIRE_TIP_COMPARISON" == "1" && "$compare_passes" -eq 0 ]]; then
  echo "ERROR: REQUIRE_TIP_COMPARISON=1 but no Haskell tip comparison passed" >&2
  echo "[forensic] compare logs: $LOG_DIR" >&2
  exit 1
fi

if [[ "$REQUIRE_TIP_COMPARISON" == "1" ]]; then
  if ! numeric_ge "$end_workers" "$EXPECT_WORKERS"; then
    echo "ERROR: REQUIRE_TIP_COMPARISON=1 but final workers_registered=$end_workers < EXPECT_WORKERS=$EXPECT_WORKERS" >&2
    echo "[forensic] metrics snapshots: $METRICS_DIR" >&2
    exit 1
  fi
  if (( worker_shortfall_samples != 0 )); then
    echo "ERROR: REQUIRE_TIP_COMPARISON=1 observed $worker_shortfall_samples post-activation worker shortfall samples" >&2
    echo "[forensic] metrics snapshots: $METRICS_DIR" >&2
    exit 1
  fi
  if (( compare_passes < MIN_TIP_COMPARE_PASSES )); then
    echo "ERROR: REQUIRE_TIP_COMPARISON=1 observed $compare_passes tip comparisons, below MIN_TIP_COMPARE_PASSES=$MIN_TIP_COMPARE_PASSES" >&2
    echo "[forensic] compare logs: $LOG_DIR" >&2
    exit 1
  fi
fi

fetch_avg="$(avg_metric "$fetch_sum" "$fetch_count")"
apply_avg="$(avg_metric "$apply_sum" "$apply_count")"

cat >"$summary_file" <<EOF
parallel_blockfetch_soak summary
network: $NETWORK
network_magic: $NETWORK_MAGIC
max_concurrent_block_fetch_peers: $MAX_CONCURRENT_BLOCK_FETCH_PEERS
expected_workers: $EXPECT_WORKERS
run_seconds: $RUN_SECONDS
blocks_synced: $start_blocks -> $end_blocks
current_slot: $start_slot -> $end_slot
reconnects: $start_reconnects -> $end_reconnects
max_workers_registered: $max_workers
workers_registered_final: $end_workers
workers_migrated_total: $migrated_total
worker_shortfall_samples: $worker_shortfall_samples
fetch_avg_per_batch: $fetch_avg
apply_avg_per_batch: $apply_avg
min_tip_compare_passes: $MIN_TIP_COMPARE_PASSES
tip_query_timeout_seconds: $TIP_QUERY_TIMEOUT_SECONDS
tip_compare_passes: $compare_passes
node_log: $node_log
metrics_dir: $METRICS_DIR
summary_json: $summary_json
EOF

write_summary_json "$summary_json"
cat "$summary_file"
echo "[info] wrote machine-readable summary: $summary_json"
echo "[ok] parallel BlockFetch soak passed"
