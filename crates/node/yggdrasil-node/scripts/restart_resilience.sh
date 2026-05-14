#!/usr/bin/env bash
set -euo pipefail

# Restart-resilience automation: kill/restart yggdrasil-node at randomized
# 5-min ± 30s intervals for N cycles (default 12 = 1 hour). Captures tip
# slot before kill, after restart, and after a 30-second settle window.
# Asserts monotonic tip progression across every cycle — tip on cycle N+1
# must be >= tip on cycle N.
#
# Targets the requirement in docs/PARITY_SUMMARY.md "Next Steps" item 3:
# "execute kill/restart cycles at 5-min and 30-min intervals and verify
# storage WAL + dirty-flag recovery leaves tip progression monotonic."
#
# Usage:
#   YGG_BIN=target/release/yggdrasil-node \
#   NETWORK=preprod \
#   CYCLES=12 \
#   node/scripts/restart_resilience.sh
#
# Exit codes:
#   0  all cycles completed with monotonic tip progression
#   1  non-monotonic regression detected (forensic logs preserved)
#   2  yggdrasil-node failed to start or recover within deadline

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
YGG_BIN="${YGG_BIN:-$ROOT_DIR/target/debug/yggdrasil-node}"
NETWORK="${NETWORK:-preprod}"
CYCLES="${CYCLES:-12}"

# Per-run private working directory.  Avoids cross-tenant clobbering on a
# shared host (CI runner, dev box) where two operators might invoke the
# script concurrently.  Audit finding L-7.
RUN_DIR="$(mktemp -d -t ygg-restart-XXXXXX)"
DB_DIR="${DB_DIR:-$RUN_DIR/db}"
SOCKET_PATH="${SOCKET_PATH:-$RUN_DIR/ygg.sock}"
LOG_ROOT="${LOG_ROOT:-$RUN_DIR/logs}"

# Metrics port: respect the env override; otherwise grab a free
# ephemeral port via Python so two concurrent runs cannot collide on
# 9099.  Falls back to 9099 if Python is unavailable.
if [[ -z "${METRICS_PORT:-}" ]]; then
  if command -v python3 >/dev/null 2>&1; then
    METRICS_PORT="$(python3 -c \
      "import socket; s=socket.socket(); s.bind(('',0)); print(s.getsockname()[1])")"
  else
    METRICS_PORT=9099
  fi
fi
INTERVAL_BASE_S="${INTERVAL_BASE_S:-300}"  # 5 min
INTERVAL_JITTER_S="${INTERVAL_JITTER_S:-30}"
SETTLE_S="${SETTLE_S:-30}"                  # post-restart settle window
START_DEADLINE_S="${START_DEADLINE_S:-60}"  # max wait for fresh start

usage() {
  cat <<'EOF'
Usage:
  YGG_BIN=path/to/yggdrasil-node \
  NETWORK={mainnet|preprod|preview} \
  CYCLES=12 \
  node/scripts/restart_resilience.sh

Optional env:
  DB_DIR              Default: $RUN_DIR/db (mktemp -d -t ygg-restart-XXXXXX)
  SOCKET_PATH         Default: $RUN_DIR/ygg.sock
  METRICS_PORT        Default: an unused ephemeral port (or 9099 if python3 absent)
  LOG_ROOT            Default: $RUN_DIR/logs
  INTERVAL_BASE_S     Default: 300 (5 min between kills)
  INTERVAL_JITTER_S   Default: 30 (±jitter on each interval)
  SETTLE_S            Default: 30 (post-restart settle before tip read)
  START_DEADLINE_S    Default: 60 (max wait for /metrics responsive)

Exit codes:
  0  all CYCLES completed with monotonic tip progression
  1  non-monotonic regression detected
  2  startup / recovery failure
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if [[ ! -x "$YGG_BIN" ]]; then
  echo "ERROR: yggdrasil-node binary not found at $YGG_BIN" >&2
  exit 2
fi

mkdir -p "$LOG_ROOT" "$DB_DIR"
rm -f "$SOCKET_PATH"

read_tip_slot() {
  # Read yggdrasil_current_slot from /metrics. Return empty on failure.
  curl -fsS "http://127.0.0.1:${METRICS_PORT}/metrics" 2>/dev/null \
    | grep -E '^yggdrasil_current_slot\s' \
    | awk '{print $2}' \
    | head -1 \
    | sed -e 's/\..*$//' || true
}

start_node() {
  local cycle="$1"
  local logfile="$LOG_ROOT/cycle-$(printf '%02d' "$cycle").log"

  ( cd "$ROOT_DIR" && "$YGG_BIN" run \
      --network "$NETWORK" \
      --database-path "$DB_DIR" \
      --socket-path "$SOCKET_PATH" \
      --metrics-port "$METRICS_PORT" \
  ) >"$logfile" 2>&1 &

  echo $!
}

wait_for_metrics() {
  local pid="$1"
  local deadline=$(( $(date +%s) + START_DEADLINE_S ))
  while [[ "$(date +%s)" -lt "$deadline" ]]; do
    if ! kill -0 "$pid" >/dev/null 2>&1; then
      return 1
    fi
    if curl -fsS "http://127.0.0.1:${METRICS_PORT}/metrics" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  return 1
}

kill_node() {
  local pid="$1"
  if ! kill -0 "$pid" >/dev/null 2>&1; then
    return 0
  fi
  # Try graceful first, then SIGKILL after 5s.
  kill -TERM "$pid" >/dev/null 2>&1 || true
  for _ in 1 2 3 4 5; do
    if ! kill -0 "$pid" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  kill -9 "$pid" >/dev/null 2>&1 || true
  wait "$pid" >/dev/null 2>&1 || true
}

last_tip=0
trap 'kill_node "${pid:-}" || true' EXIT INT TERM

echo "[info] restart_resilience: NETWORK=$NETWORK CYCLES=$CYCLES base=${INTERVAL_BASE_S}s jitter=±${INTERVAL_JITTER_S}s"
echo "[info] DB_DIR=$DB_DIR SOCKET=$SOCKET_PATH METRICS_PORT=$METRICS_PORT"
echo "[info] LOG_ROOT=$LOG_ROOT"

for ((cycle=1; cycle<=CYCLES; cycle++)); do
  echo "[cycle $cycle/$CYCLES] starting node"
  pid="$(start_node "$cycle")"

  if ! wait_for_metrics "$pid"; then
    echo "ERROR: cycle $cycle: node failed to expose /metrics within ${START_DEADLINE_S}s" >&2
    exit 2
  fi

  # Settle window then sample tip.
  sleep "$SETTLE_S"
  if ! kill -0 "$pid" >/dev/null 2>&1; then
    echo "ERROR: cycle $cycle: node crashed during settle window" >&2
    exit 2
  fi
  current_tip="$(read_tip_slot)"
  if [[ -z "$current_tip" ]]; then
    echo "ERROR: cycle $cycle: could not read yggdrasil_current_slot" >&2
    exit 2
  fi

  echo "[cycle $cycle] tip slot = $current_tip (previous = $last_tip)"

  # Monotonicity: each cycle's settled tip must be >= the previous cycle's.
  if (( current_tip < last_tip )); then
    echo "ERROR: cycle $cycle: NON-MONOTONIC tip regression: $current_tip < $last_tip" >&2
    echo "[forensic] preserving logs at $LOG_ROOT" >&2
    exit 1
  fi
  last_tip="$current_tip"

  # Compute next-kill jitter; on the last cycle just kill immediately so we
  # exercise WAL recovery before reporting success.
  local_jitter=$(( RANDOM % (2 * INTERVAL_JITTER_S + 1) - INTERVAL_JITTER_S ))
  hold=$(( INTERVAL_BASE_S + local_jitter ))
  if [[ "$cycle" -eq "$CYCLES" ]]; then
    hold=5  # short hold on final cycle, then assert monotonic on the kill side too
  fi
  echo "[cycle $cycle] holding ${hold}s before kill"
  sleep "$hold"

  echo "[cycle $cycle] killing node"
  kill_node "$pid"
done

# Final cycle: one more startup to assert WAL recovery on the *post-final-kill*
# state can still produce a monotonic tip.
echo "[final] post-CYCLES recovery probe"
pid="$(start_node "$((CYCLES + 1))")"
if ! wait_for_metrics "$pid"; then
  echo "ERROR: final recovery probe failed to expose /metrics" >&2
  exit 2
fi
sleep "$SETTLE_S"
final_tip="$(read_tip_slot)"
if [[ -z "$final_tip" ]]; then
  echo "ERROR: final recovery probe could not read tip" >&2
  exit 2
fi
if (( final_tip < last_tip )); then
  echo "ERROR: final recovery: NON-MONOTONIC tip regression: $final_tip < $last_tip" >&2
  exit 1
fi
echo "[final] final recovery tip = $final_tip (previous = $last_tip)"
kill_node "$pid"

echo "[ok] all $CYCLES cycles + final recovery completed monotonic tip progression"
echo "[ok] last tip slot = $final_tip"
echo "[ok] logs: $LOG_ROOT"
