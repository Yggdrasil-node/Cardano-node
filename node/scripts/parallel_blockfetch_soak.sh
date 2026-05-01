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
#   node/scripts/parallel_blockfetch_soak.sh
#
# Exit codes:
#   0  soak completed and all enabled assertions passed
#   1  parity or liveness assertion failed
#   2  node startup, metrics, or invocation failure

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

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
START_DEADLINE_S="${START_DEADLINE_S:-90}"
CARDANO_CLI="${CARDANO_CLI:-cardano-cli}"
HASKELL_SOCK="${HASKELL_SOCK:-}"

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
  if command -v python3 >/dev/null 2>&1; then
    METRICS_PORT="$(python3 -c \
      "import socket; s=socket.socket(); s.bind(('',0)); print(s.getsockname()[1])")"
  else
    METRICS_PORT=9201
  fi
fi

METRICS_URL="http://127.0.0.1:${METRICS_PORT}/metrics"

usage() {
  cat <<'EOF'
Usage:
  NETWORK={mainnet|preprod|preview} \
  MAX_CONCURRENT_BLOCK_FETCH_PEERS=2 \
  RUN_SECONDS=21600 \
  node/scripts/parallel_blockfetch_soak.sh

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
  EXPECT_WORKERS                    Default: MAX_CONCURRENT_BLOCK_FETCH_PEERS
  REQUIRE_WORKERS                   Default: 1. Set 0 only for diagnostic captures.
  REQUIRE_PROGRESS                  Default: 1. Set 0 only when attaching to a deliberately idle network.

Exit codes:
  0  soak completed and all enabled assertions passed
  1  parity/liveness assertion failed
  2  startup, metrics, or invocation failure
EOF
}

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

require_uint "MAX_CONCURRENT_BLOCK_FETCH_PEERS" "$MAX_CONCURRENT_BLOCK_FETCH_PEERS"
require_uint "EXPECT_WORKERS" "$EXPECT_WORKERS"
require_uint "REQUIRE_WORKERS" "$REQUIRE_WORKERS"
require_uint "REQUIRE_PROGRESS" "$REQUIRE_PROGRESS"
require_uint "RUN_SECONDS" "$RUN_SECONDS"
require_uint "SAMPLE_INTERVAL_S" "$SAMPLE_INTERVAL_S"
require_uint "COMPARE_INTERVAL_S" "$COMPARE_INTERVAL_S"
require_uint "START_DEADLINE_S" "$START_DEADLINE_S"

if (( MAX_CONCURRENT_BLOCK_FETCH_PEERS < 2 )); then
  echo "ERROR: MAX_CONCURRENT_BLOCK_FETCH_PEERS must be >= 2 for parallel BlockFetch rehearsal" >&2
  exit 2
fi

if (( SAMPLE_INTERVAL_S == 0 )); then
  echo "ERROR: SAMPLE_INTERVAL_S must be > 0" >&2
  exit 2
fi

if [[ ! -x "$YGG_BIN" ]]; then
  echo "ERROR: yggdrasil-node binary not found at $YGG_BIN" >&2
  exit 2
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "ERROR: curl is required for Prometheus metrics sampling" >&2
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

mkdir -p "$DB_DIR" "$LOG_DIR" "$METRICS_DIR"
rm -f "$SOCKET_PATH"

node_log="$LOG_DIR/yggdrasil-node.log"
summary_file="$LOG_DIR/summary.txt"
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
    "$ROOT_DIR/node/scripts/compare_tip_to_haskell.sh" >"$logfile" 2>&1
}

echo "[info] parallel_blockfetch_soak: NETWORK=$NETWORK magic=$NETWORK_MAGIC knob=$MAX_CONCURRENT_BLOCK_FETCH_PEERS expected_workers=$EXPECT_WORKERS"
echo "[info] RUN_SECONDS=$RUN_SECONDS SAMPLE_INTERVAL_S=$SAMPLE_INTERVAL_S COMPARE_INTERVAL_S=$COMPARE_INTERVAL_S"
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
fetch_avg_per_batch: $fetch_avg
apply_avg_per_batch: $apply_avg
tip_compare_passes: $compare_passes
node_log: $node_log
metrics_dir: $METRICS_DIR
EOF

cat "$summary_file"
echo "[ok] parallel BlockFetch soak passed"
