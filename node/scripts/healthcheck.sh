#!/usr/bin/env bash
# healthcheck.sh — Yggdrasil node health probe.
#
# Reports an operator-friendly summary: process uptime, current slot,
# block number, blocks synced, mempool size, hot peers, recent reconnects.
# Exits 0 if the node is reachable and not stalled, 1 otherwise.
#
# Usage:
#   ./healthcheck.sh                              # localhost:12798
#   ./healthcheck.sh http://10.0.0.5:12798        # custom endpoint
#   ./healthcheck.sh http://127.0.0.1:12798 --quiet  # exit-code only

set -euo pipefail

ENDPOINT="${1:-http://127.0.0.1:12798}"
QUIET=0
for arg in "$@"; do
  [ "$arg" = "--quiet" ] && QUIET=1
done

err()  { [ "$QUIET" = "1" ] || printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }
info() { [ "$QUIET" = "1" ] || printf '%s\n' "$*"; }

command -v curl >/dev/null 2>&1 || err "curl is required"

# Pull /health for liveness, /metrics for full state.
health_json="$(curl -fsS --max-time 5 "${ENDPOINT}/health")" \
  || err "node is not reachable at ${ENDPOINT}/health"

# Extract values without requiring jq.
extract() { echo "$health_json" | grep -oE "\"$1\"[[:space:]]*:[[:space:]]*[^,}]*" | sed 's/.*://; s/[" ]//g'; }
status=$(extract status)
uptime=$(extract uptime_seconds)
blocks=$(extract blocks_synced)
slot=$(extract current_slot)

[ "$status" = "ok" ] || err "node reports status: ${status:-unknown}"

# Pull a couple of metrics for context.
metrics_text="$(curl -fsS --max-time 5 "${ENDPOINT}/metrics" 2>/dev/null || true)"
metric() { echo "$metrics_text" | awk -v m="$1" '$1 == m { print $2 }' | head -1; }
mempool=$(metric yggdrasil_mempool_tx_count)
outbound=$(metric yggdrasil_cm_outbound_conns)
reconnects=$(metric yggdrasil_reconnects_total)
workers=$(metric yggdrasil_blockfetch_workers_registered)

if [ "$QUIET" = "0" ]; then
  cat <<EOF
yggdrasil-node @ ${ENDPOINT}
  status:           ${status}
  uptime:           ${uptime}s
  current slot:     ${slot}
  blocks synced:    ${blocks}
  mempool tx:       ${mempool:-?}
  outbound peers:   ${outbound:-?}
  fetch workers:    ${workers:-?}
  reconnects total: ${reconnects:-?}
EOF
fi

# Fail if we have zero outbound peers AND uptime > 5 minutes — likely stalled.
if [ "${uptime:-0}" -gt 300 ] 2>/dev/null && [ "${outbound:-0}" = "0" ]; then
  err "node has 0 outbound peers after ${uptime}s of uptime"
fi

exit 0
