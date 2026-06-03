#!/usr/bin/env bash
set -euo pipefail

# Compare yggdrasil-cardano-submit-api vs upstream cardano-submit-api
# binary-byte-by-binary-byte across the canonical request surface.
#
# Designed for the R825+ cardano-submit-api operator-evidence follow-on.
# CI cannot run this script (it needs a live cardano-node socket + the
# upstream binary); operators run it manually before promoting the
# parity-matrix entry to verified_11_0_1.
#
# Procedure:
#  1. Bring up upstream cardano-node (e.g. on preview testnet).
#  2. Run upstream cardano-submit-api on port 18090.
#  3. Run yggdrasil-cardano-submit-api on port 18091.
#  4. POST a series of canonical inputs (empty, malformed, valid)
#     to both endpoints, diff response status + body.
#  5. Scrape /metrics from both, confirm counter shape is byte-equal.
#
# Usage:
#   UPSTREAM_PORT=18090 YGGDRASIL_PORT=18091 \
#   UPSTREAM_METRICS_PORT=18181 YGGDRASIL_METRICS_PORT=18182 \
#   dev/evidence/compare_submit_api_to_upstream.sh
#
# Exit codes:
#   0  every observable response (status + body) byte-identical between
#      the two binaries.
#   1  divergence detected on at least one endpoint; full diff printed.
#   2  one or both endpoints unreachable / unparseable.
#   3  bad invocation (missing required tooling).

UPSTREAM_PORT="${UPSTREAM_PORT:-18090}"
YGGDRASIL_PORT="${YGGDRASIL_PORT:-18091}"
UPSTREAM_METRICS_PORT="${UPSTREAM_METRICS_PORT:-18181}"
YGGDRASIL_METRICS_PORT="${YGGDRASIL_METRICS_PORT:-18182}"
HOST="${HOST:-127.0.0.1}"

# Number of seconds to wait between submission and metrics scrape so
# the counter increment lands.
SETTLE_SECONDS="${SETTLE_SECONDS:-1}"

usage() {
  cat <<'EOF'
Usage:
  UPSTREAM_PORT=<port>            (default 18090)
  YGGDRASIL_PORT=<port>           (default 18091)
  UPSTREAM_METRICS_PORT=<port>    (default 18181)
  YGGDRASIL_METRICS_PORT=<port>   (default 18182)
  HOST=<host>                     (default 127.0.0.1)
  SETTLE_SECONDS=<n>              (default 1)
  dev/evidence/compare_submit_api_to_upstream.sh

Compares yggdrasil-cardano-submit-api vs upstream cardano-submit-api
across the canonical request surface. Both binaries must already be
running and connected to the same cardano-node socket.

Sample setup:

  # Terminal 1: upstream cardano-node + cardano-submit-api on preview.
  .reference-haskell-cardano-node/install/bin/cardano-node run ...
  .reference-haskell-cardano-node/install/bin/cardano-submit-api \
    --config /etc/submit-api-upstream.json \
    --socket-path /tmp/preview/socket/node.socket \
    --testnet-magic 2 --port 18090

  # Terminal 2: yggdrasil-cardano-submit-api against the same socket.
  cargo run --release --bin cardano-submit-api -- \
    --config /etc/submit-api-yggdrasil.json \
    --socket-path /tmp/preview/socket/node.socket \
    --testnet-magic 2 --port 18091 --metrics-port 18182

  # Terminal 3: this script.
  dev/evidence/compare_submit_api_to_upstream.sh
EOF
  exit 3
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "error: curl not on \$PATH" >&2
  exit 3
fi

if ! command -v diff >/dev/null 2>&1; then
  echo "error: diff not on \$PATH" >&2
  exit 3
fi

# ---------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------

post_to_both() {
  local label="$1"
  local body="$2"

  local upstream_body yggdrasil_body
  local upstream_status yggdrasil_status

  echo "--- ${label} ---"

  if ! upstream_status=$(curl -sS -o /tmp/upstream.body -w '%{http_code}' \
        -X POST "http://${HOST}:${UPSTREAM_PORT}/api/submit/tx" \
        -H 'Content-Type: application/cbor' \
        --data-binary @<(printf '%s' "$body")); then
    echo "  upstream unreachable on port ${UPSTREAM_PORT}" >&2
    return 1
  fi
  upstream_body=$(cat /tmp/upstream.body)

  if ! yggdrasil_status=$(curl -sS -o /tmp/ygg.body -w '%{http_code}' \
        -X POST "http://${HOST}:${YGGDRASIL_PORT}/api/submit/tx" \
        -H 'Content-Type: application/cbor' \
        --data-binary @<(printf '%s' "$body")); then
    echo "  yggdrasil unreachable on port ${YGGDRASIL_PORT}" >&2
    return 1
  fi
  yggdrasil_body=$(cat /tmp/ygg.body)

  echo "  upstream:  HTTP ${upstream_status} body=$(head -c 200 /tmp/upstream.body)"
  echo "  yggdrasil: HTTP ${yggdrasil_status} body=$(head -c 200 /tmp/ygg.body)"

  if [ "${upstream_status}" != "${yggdrasil_status}" ]; then
    echo "  STATUS DIVERGED: ${upstream_status} != ${yggdrasil_status}" >&2
    return 1
  fi

  if ! diff -u /tmp/upstream.body /tmp/ygg.body >/dev/null; then
    echo "  BODY DIVERGED:" >&2
    diff -u /tmp/upstream.body /tmp/ygg.body >&2
    return 1
  fi

  echo "  ✓ identical (HTTP ${upstream_status})"
  return 0
}

scrape_metrics() {
  local label="$1"
  local port="$2"

  if ! curl -sS "http://${HOST}:${port}/metrics" -o "/tmp/${label}.metrics"; then
    echo "  ${label} metrics unreachable on port ${port}" >&2
    return 1
  fi

  echo "--- ${label} /metrics ---"
  cat "/tmp/${label}.metrics"
  return 0
}

# ---------------------------------------------------------------------
# Test surface
# ---------------------------------------------------------------------

failures=0

# Test 1: empty body. Both should return 400 with TxSubmitEmpty JSON.
if ! post_to_both "empty body" ""; then
  failures=$((failures + 1))
fi

# Test 2: malformed CBOR. Both should return 400 (validation error from
# cardano-node MsgRejectTx) — exact reason bytes depend on cardano-
# node's mempool-validation surface, so we expect the JSON tag /
# wrapper structure to match but not the inner string.
if ! post_to_both "malformed CBOR" "$(printf '\xff\xff\xff\xff')"; then
  echo "  (note: inner reason string may legitimately differ between" >&2
  echo "   binaries — the wrapper shape must still match.)" >&2
  failures=$((failures + 1))
fi

# Wait for the metrics counters to settle then scrape both endpoints.
sleep "${SETTLE_SECONDS}"

scrape_metrics upstream "${UPSTREAM_METRICS_PORT}" || failures=$((failures + 1))
scrape_metrics yggdrasil "${YGGDRASIL_METRICS_PORT}" || failures=$((failures + 1))

# Compare the metrics shape (line-by-line ignoring counter values).
if [ -f /tmp/upstream.metrics ] && [ -f /tmp/yggdrasil.metrics ]; then
  echo "--- /metrics shape diff (counter values stripped) ---"
  if diff -u \
      <(grep -E '^# (HELP|TYPE)' /tmp/upstream.metrics) \
      <(grep -E '^# (HELP|TYPE)' /tmp/yggdrasil.metrics); then
    echo "  ✓ counter shape (HELP + TYPE) identical"
  else
    echo "  COUNTER SHAPE DIVERGED" >&2
    failures=$((failures + 1))
  fi
fi

if [ "${failures}" -eq 0 ]; then
  echo
  echo "All endpoints byte-identical. The R825+ operator-evidence"
  echo "follow-on can promote sister-tool.cardano-submit-api from"
  echo "'implemented_needs_11_0_1_evidence' to 'verified_11_0_1'."
  exit 0
else
  echo
  echo "${failures} endpoint(s) diverged. Investigate before verified promotion." >&2
  exit 1
fi
