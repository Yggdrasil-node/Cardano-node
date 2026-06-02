#!/usr/bin/env bash
set -euo pipefail

# Run yggdrasil-node as a preview block producer using real preview pool
# credentials. This intentionally does not use preview_producer_harness.sh
# generated credentials; all producer credential paths come from env vars.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="${LOG_DIR:-/tmp/ygg-real-preview}"
DB_DIR="${DB_DIR:-/tmp/ygg-preview-real-bp-db}"
SOCKET_PATH="${SOCKET_PATH:-/tmp/ygg-preview-real-bp.sock}"
METRICS_PORT="${METRICS_PORT:-19002}"
RUN_SECONDS="${RUN_SECONDS:-600}"
EXPECT_FORGE_EVENTS="${EXPECT_FORGE_EVENTS:-0}"
EXPECT_ADOPTED_EVENTS="${EXPECT_ADOPTED_EVENTS:-0}"
TIP_COMPARE_CHECKPOINTS="${TIP_COMPARE_CHECKPOINTS:-900,3600,21600}"
REQUIRE_TIP_COMPARISON="${REQUIRE_TIP_COMPARISON:-0}"
HASKELL_SOCK="${HASKELL_SOCK:-}"
SNAPSHOT_DIR="${SNAPSHOT_DIR:-$LOG_DIR/tip-snapshots}"
METRICS_DIR="${METRICS_DIR:-$LOG_DIR/metrics}"
METRICS_SNAPSHOT_INTERVAL_S="${METRICS_SNAPSHOT_INTERVAL_S:-60}"

if [[ -z "${CARDANO_CLI:-}" ]]; then
  if [[ -x "$ROOT_DIR/.reference-haskell-cardano-node/install/bin/cardano-cli" ]]; then
    CARDANO_CLI="$ROOT_DIR/.reference-haskell-cardano-node/install/bin/cardano-cli"
  else
    CARDANO_CLI="cardano-cli"
  fi
fi

if [[ -z "${YGG_BIN:-}" ]]; then
  if [[ -x "$ROOT_DIR/target/release/yggdrasil-node" ]]; then
    YGG_BIN="$ROOT_DIR/target/release/yggdrasil-node"
  else
    YGG_BIN="$ROOT_DIR/target/debug/yggdrasil-node"
  fi
fi

KES_SKEY_PATH_VALUE="${KES_SKEY_PATH:-}"
VRF_SKEY_PATH="${VRF_SKEY_PATH:-}"
OPCERT_PATH="${OPCERT_PATH:-}"

usage() {
  cat <<'EOF'
Usage:
  KES_SKEY_PATH=/abs/path/kes.skey \
  VRF_SKEY_PATH=/abs/path/vrf.skey \
  OPCERT_PATH=/abs/path/node.cert \
  scripts/run_preview_real_pool_producer.sh

Runs:
  yggdrasil-node validate-config --network preview ...
  yggdrasil-node run --network preview ...

Optional env:
  YGG_BIN                Default: target/release/yggdrasil-node if present, else target/debug/yggdrasil-node
  LOG_DIR                Default: /tmp/ygg-real-preview
  DB_DIR                 Default: /tmp/ygg-preview-real-bp-db
  SOCKET_PATH            Default: /tmp/ygg-preview-real-bp.sock
  METRICS_PORT           Default: 19002
  RUN_SECONDS            Default: 600
  EXPECT_FORGE_EVENTS    Default: 0 (set 1 to require leader/forge evidence)
  EXPECT_ADOPTED_EVENTS  Default: 0 (set 1 to require adopted forged block)
  HASKELL_SOCK           Optional cardano-node preview socket; enables tip comparison
  CARDANO_CLI            Default: cardano-cli
  SNAPSHOT_DIR           Default: $LOG_DIR/tip-snapshots
  METRICS_DIR            Default: $LOG_DIR/metrics
  METRICS_SNAPSHOT_INTERVAL_S Default: 60
  TIP_COMPARE_CHECKPOINTS Default: 900,3600,21600 (15m, 60m, 6h)
  REQUIRE_TIP_COMPARISON Default: 0 (set 1 to require every configured checkpoint)

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
    echo "Hint: run 'cargo build --release -p yggdrasil-node' first." >&2
    return 1
  fi
  if ! command -v curl >/dev/null 2>&1; then
    echo "ERROR: curl is required for metrics snapshot capture" >&2
    return 1
  fi
  if ! command -v python3 >/dev/null 2>&1; then
    echo "ERROR: python3 is required to validate the validate-config JSON report" >&2
    return 1
  fi
  if [[ -n "$HASKELL_SOCK" ]] && ! command -v "$CARDANO_CLI" >/dev/null 2>&1; then
    echo "ERROR: cardano-cli not found in PATH (set CARDANO_CLI=/abs/path)" >&2
    return 1
  fi
  if [[ -n "$HASKELL_SOCK" && ! -S "$HASKELL_SOCK" ]]; then
    echo "ERROR: HASKELL_SOCK is not a unix socket: $HASKELL_SOCK" >&2
    return 1
  fi
  if [[ -n "$HASKELL_SOCK" ]]; then
    if ! CARDANO_NODE_SOCKET_PATH="$HASKELL_SOCK" \
      "$CARDANO_CLI" query tip --testnet-magic 2 \
      >/dev/null 2>&1; then
      echo "ERROR: failed to query preview tip through HASKELL_SOCK=$HASKELL_SOCK" >&2
      echo "Hint: start the upstream preview relay and pass its preview socket path." >&2
      return 1
    fi
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

sample_metrics() {
  local label="$1"
  local ts file
  ts="$(date -u +%Y%m%dT%H%M%SZ)"
  file="$METRICS_DIR/${label}-${ts}.prom"
  if curl -fsS "http://127.0.0.1:${METRICS_PORT}/metrics" >"$file" 2>/dev/null; then
    echo "[info] metrics snapshot: $file"
    return 0
  fi
  rm -f "$file"
  return 1
}

parse_tip_checkpoints() {
  local raw="$1"
  local -n out_ref="$2"
  local item

  out_ref=()
  IFS=',' read -ra out_ref <<<"$raw"
  for item in "${out_ref[@]}"; do
    if [[ ! "$item" =~ ^[0-9]+$ || "$item" == "0" ]]; then
      echo "ERROR: TIP_COMPARE_CHECKPOINTS must be comma-separated positive seconds, got '$raw'" >&2
      return 1
    fi
  done
}

require_positive_uint() {
  local name="$1"
  local value="$2"
  if [[ ! "$value" =~ ^[0-9]+$ || "$value" == "0" ]]; then
    echo "ERROR: $name must be a positive integer, got '$value'" >&2
    return 1
  fi
}

require_bool01() {
  local name="$1"
  local value="$2"
  if [[ "$value" != "0" && "$value" != "1" ]]; then
    echo "ERROR: $name must be 0 or 1, got '$value'" >&2
    return 1
  fi
}

assert_validate_report() {
  local validate_file="$1"
  python3 - "$validate_file" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8") as f:
    report = json.load(f)

node_role = report.get("node_role") or {}
required_present = {
    "ShelleyKesKey",
    "ShelleyVrfKey",
    "ShelleyOperationalCertificate",
}
present = set(node_role.get("credential_fields_present") or [])
missing = set(node_role.get("credential_fields_missing") or [])
errors = []

if node_role.get("role") != "block-producer":
    errors.append(f"node_role.role={node_role.get('role')!r}, expected 'block-producer'")
if node_role.get("non_producing_node") is not False:
    errors.append(
        f"node_role.non_producing_node={node_role.get('non_producing_node')!r}, expected false"
    )
if node_role.get("block_producer_credentials") != "complete":
    errors.append(
        "node_role.block_producer_credentials="
        f"{node_role.get('block_producer_credentials')!r}, expected 'complete'"
    )
if present != required_present:
    errors.append(
        "credential_fields_present="
        f"{sorted(present)!r}, expected {sorted(required_present)!r}"
    )
if missing:
    errors.append(f"credential_fields_missing={sorted(missing)!r}, expected []")

if errors:
    print("ERROR: validate-config report did not confirm preview producer credentials:", file=sys.stderr)
    for error in errors:
        print(f"  - {error}", file=sys.stderr)
    sys.exit(1)
PY
}

run_tip_comparison() {
  local checkpoint="$1"
  local log_file="$2"
  local run_id="$3"
  local compare_file="$LOG_DIR/tip-comparison-${run_id}-${checkpoint}s.log"

  echo "[info] running Haskell tip comparison checkpoint=${checkpoint}s -> $compare_file"
  if ! YGG_BIN="$YGG_BIN" \
    YGG_SOCK="$SOCKET_PATH" \
    HASKELL_SOCK="$HASKELL_SOCK" \
    NETWORK_MAGIC=2 \
    CARDANO_CLI="$CARDANO_CLI" \
    SNAPSHOT_DIR="$SNAPSHOT_DIR" \
    "$ROOT_DIR/scripts/compare_tip_to_haskell.sh" \
    >"$compare_file" 2>&1; then
    echo "ERROR: Haskell tip comparison failed at checkpoint=${checkpoint}s" >&2
    echo "[info] comparison log:" >&2
    cat "$compare_file" >&2 || true
    echo "[info] last producer log lines:" >&2
    tail -n 80 "$log_file" >&2 || true
    return 1
  fi
}

write_summary() {
  local summary_file="$1"
  local validate_file="$2"
  local log_file="$3"
  local metrics_snapshots="$4"
  local tip_comparisons_run="$5"
  local tip_comparisons_expected="$6"
  local leader_count forged_count adopted_count not_adopted_count

  leader_count="$(grep -c "elected as slot leader" "$log_file" || true)"
  forged_count="$(grep -c "forged local block" "$log_file" || true)"
  adopted_count="$(grep -c "adopted forged block" "$log_file" || true)"
  not_adopted_count="$(grep -c "did not adopt forged block" "$log_file" || true)"

  cat >"$summary_file" <<EOF
preview_real_pool_producer summary
network: preview
yggdrasil_node: $YGG_BIN
run_seconds: $RUN_SECONDS
database_path: $DB_DIR
socket_path: $SOCKET_PATH
metrics_port: $METRICS_PORT
metrics_dir: $METRICS_DIR
metrics_snapshots: $metrics_snapshots
validate_file: $validate_file
log_file: $log_file
haskell_sock: ${HASKELL_SOCK:-disabled}
tip_compare_checkpoints: $TIP_COMPARE_CHECKPOINTS
tip_comparisons_expected: $tip_comparisons_expected
tip_comparisons_run: $tip_comparisons_run
expect_forge_events: $EXPECT_FORGE_EVENTS
expect_adopted_events: $EXPECT_ADOPTED_EVENTS
leaders: $leader_count
forged: $forged_count
adopted: $adopted_count
not_adopted: $not_adopted_count
EOF
}

main() {
  if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
  fi

  require_file "$KES_SKEY_PATH_VALUE" "KES_SKEY_PATH"
  require_file "$VRF_SKEY_PATH" "VRF_SKEY_PATH"
  require_file "$OPCERT_PATH" "OPCERT_PATH"
  require_positive_uint "RUN_SECONDS" "$RUN_SECONDS"
  require_positive_uint "METRICS_PORT" "$METRICS_PORT"
  require_positive_uint "METRICS_SNAPSHOT_INTERVAL_S" "$METRICS_SNAPSHOT_INTERVAL_S"
  require_bool01 "EXPECT_FORGE_EVENTS" "$EXPECT_FORGE_EVENTS"
  require_bool01 "EXPECT_ADOPTED_EVENTS" "$EXPECT_ADOPTED_EVENTS"
  require_bool01 "REQUIRE_TIP_COMPARISON" "$REQUIRE_TIP_COMPARISON"
  ensure_tools

  local tip_checkpoints=()
  parse_tip_checkpoints "$TIP_COMPARE_CHECKPOINTS" tip_checkpoints
  if [[ "$REQUIRE_TIP_COMPARISON" == "1" && -z "$HASKELL_SOCK" ]]; then
    echo "ERROR: REQUIRE_TIP_COMPARISON=1 requires HASKELL_SOCK" >&2
    exit 1
  fi
  if [[ -n "$HASKELL_SOCK" ]]; then
    local checkpoint
    for checkpoint in "${tip_checkpoints[@]}"; do
      if [[ "$checkpoint" -le "$RUN_SECONDS" ]]; then
        continue
      fi
      if [[ "$REQUIRE_TIP_COMPARISON" == "1" ]]; then
        echo "ERROR: REQUIRE_TIP_COMPARISON=1 but checkpoint ${checkpoint}s exceeds RUN_SECONDS=$RUN_SECONDS" >&2
        exit 1
      fi
    done
    if [[ "$REQUIRE_TIP_COMPARISON" != "1" && "${#tip_checkpoints[@]}" -gt 0 ]]; then
      local reachable_checkpoint=0
      for checkpoint in "${tip_checkpoints[@]}"; do
        if [[ "$checkpoint" -le "$RUN_SECONDS" ]]; then
          reachable_checkpoint=1
          break
        fi
      done
      if [[ "$reachable_checkpoint" == "0" ]]; then
        echo "ERROR: HASKELL_SOCK set but no TIP_COMPARE_CHECKPOINTS fall within RUN_SECONDS=$RUN_SECONDS" >&2
        exit 1
      fi
    elif [[ "${#tip_checkpoints[@]}" -eq 0 ]]; then
      echo "ERROR: HASKELL_SOCK set but no TIP_COMPARE_CHECKPOINTS fall within RUN_SECONDS=$RUN_SECONDS" >&2
      exit 1
    fi
  fi

  mkdir -p "$LOG_DIR" "$DB_DIR" "$SNAPSHOT_DIR" "$METRICS_DIR"
  rm -f "$SOCKET_PATH"
  local run_id
  run_id="$(date +%Y%m%d-%H%M%S)"
  local log_file="$LOG_DIR/preview-real-pool-${run_id}.log"
  local validate_file="$LOG_DIR/preview-real-pool-validate-${run_id}.json"
  local summary_file="$LOG_DIR/preview-real-pool-summary-${run_id}.txt"
  local tip_comparisons_run=0
  local tip_comparisons_expected=0
  local metrics_snapshots=0

  if [[ -n "$HASKELL_SOCK" ]]; then
    tip_comparisons_expected="${#tip_checkpoints[@]}"
  fi

  echo "[info] yggdrasil-node: $YGG_BIN"
  echo "[info] log file:      $log_file"
  echo "[info] validate file: $validate_file"
  echo "[info] metrics port:  $METRICS_PORT"
  echo "[info] metrics dir:   $METRICS_DIR"
  echo "[info] run window:    ${RUN_SECONDS}s"
  if [[ -n "$HASKELL_SOCK" ]]; then
    echo "[info] haskell sock:  $HASKELL_SOCK"
    echo "[info] tip checkpoints: $TIP_COMPARE_CHECKPOINTS"
  fi

  "$YGG_BIN" validate-config \
    --network preview \
    --shelley-kes-key "$KES_SKEY_PATH_VALUE" \
    --shelley-vrf-key "$VRF_SKEY_PATH" \
    --shelley-operational-certificate "$OPCERT_PATH" \
    >"$validate_file"
  assert_validate_report "$validate_file"

  set +e
  (
    cd "$ROOT_DIR" &&
      "$YGG_BIN" run \
        --network preview \
        --database-path "$DB_DIR" \
        --socket-path "$SOCKET_PATH" \
        --metrics-port "$METRICS_PORT" \
        --shelley-kes-key "$KES_SKEY_PATH_VALUE" \
        --shelley-vrf-key "$VRF_SKEY_PATH" \
        --shelley-operational-certificate "$OPCERT_PATH"
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
      tail -n 80 "$log_file" >&2 || true
      exit 1
    fi
    sleep 1
    elapsed=$((elapsed + 1))
    if [[ "$elapsed" == "1" || $(( elapsed % METRICS_SNAPSHOT_INTERVAL_S )) -eq 0 ]]; then
      if sample_metrics "sample-${elapsed}s"; then
        metrics_snapshots=$((metrics_snapshots + 1))
      fi
    fi
    if [[ -n "$HASKELL_SOCK" ]]; then
      local checkpoint
      for checkpoint in "${tip_checkpoints[@]}"; do
        if [[ "$checkpoint" == "$elapsed" ]]; then
          run_tip_comparison "$checkpoint" "$log_file" "$run_id" || exit 1
          tip_comparisons_run=$((tip_comparisons_run + 1))
        fi
      done
    fi
  done

  if sample_metrics "final"; then
    metrics_snapshots=$((metrics_snapshots + 1))
  fi

  kill "$pid" >/dev/null 2>&1 || true
  wait "$pid" >/dev/null 2>&1 || true
  trap - EXIT INT TERM
  set -e

  echo "[info] verifying runtime signals..."

  if ! grep -q "Startup.BlockProducer" "$log_file"; then
    echo "ERROR: did not observe Startup.BlockProducer in logs" >&2
    exit 1
  fi
  if ! grep -q "block producer credentials loaded" "$log_file"; then
    echo "ERROR: did not observe block producer credential load" >&2
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
    echo "ERROR: did not observe preview bootstrap connection" >&2
    exit 1
  fi

  if [[ "$EXPECT_FORGE_EVENTS" == "1" ]]; then
    if ! grep -q "elected as slot leader" "$log_file"; then
      echo "ERROR: EXPECT_FORGE_EVENTS=1 but no leader election found" >&2
      echo "Hint: increase RUN_SECONDS and confirm the preview pool has active delegated stake." >&2
      exit 1
    fi
    if ! grep -q "forged local block" "$log_file"; then
      echo "ERROR: EXPECT_FORGE_EVENTS=1 but no forged local block found" >&2
      echo "Hint: increase RUN_SECONDS and confirm the preview pool has active delegated stake." >&2
      exit 1
    fi
    if ! grep -Eq "adopted forged block|did not adopt forged block" "$log_file"; then
      echo "ERROR: EXPECT_FORGE_EVENTS=1 but no forged-block adoption judgement found" >&2
      echo "Hint: increase RUN_SECONDS and confirm the preview pool has active delegated stake." >&2
      exit 1
    fi
  fi

  if [[ "$EXPECT_ADOPTED_EVENTS" == "1" ]]; then
    if ! grep -q "adopted forged block" "$log_file"; then
      echo "ERROR: EXPECT_ADOPTED_EVENTS=1 but no adopted forged block found" >&2
      echo "Hint: ensure the pool is active/registered on preview and extend RUN_SECONDS." >&2
      exit 1
    fi
  fi

  if [[ "$REQUIRE_TIP_COMPARISON" == "1" && "$tip_comparisons_run" -ne "$tip_comparisons_expected" ]]; then
    echo "ERROR: REQUIRE_TIP_COMPARISON=1 but only $tip_comparisons_run/$tip_comparisons_expected Haskell tip comparisons ran" >&2
    exit 1
  fi
  if [[ "$metrics_snapshots" -eq 0 ]]; then
    echo "ERROR: no metrics snapshots were captured from port $METRICS_PORT" >&2
    exit 1
  fi

  summarize_evidence "$log_file"
  write_summary "$summary_file" "$validate_file" "$log_file" "$metrics_snapshots" "$tip_comparisons_run" "$tip_comparisons_expected"

  echo "[ok] producer-mode preview verification checks passed"
  echo "[ok] validate: $validate_file"
  echo "[ok] log:      $log_file"
  echo "[ok] metrics:  $METRICS_DIR ($metrics_snapshots snapshots)"
  echo "[ok] summary:  $summary_file"
}

main "$@"
