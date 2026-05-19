#!/usr/bin/env bash
set -euo pipefail

# Build, sign, and optionally submit a preview stake-pool registration
# transaction for the generated preview credential bundle. This is an operator
# helper only: a signed/submitted transaction plus later on-chain observation is
# required before any producer run can claim active-pool evidence.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"

if [[ -z "${CARDANO_CLI:-}" ]]; then
  if [[ -x "$ROOT_DIR/.reference-haskell-cardano-node/install/bin/cardano-cli" ]]; then
    CARDANO_CLI="$ROOT_DIR/.reference-haskell-cardano-node/install/bin/cardano-cli"
  else
    CARDANO_CLI="cardano-cli"
  fi
fi

CRED_DIR="${CRED_DIR:-}"
REGISTRATION_DIR="${REGISTRATION_DIR:-${CRED_DIR:+$CRED_DIR/registration}}"
SOCKET_PATH="${SOCKET_PATH:-${CARDANO_NODE_SOCKET_PATH:-}}"
NETWORK_MAGIC="${NETWORK_MAGIC:-2}"
SUBMIT="${SUBMIT:-0}"
KOIOS_SUBMIT="${KOIOS_SUBMIT:-0}"
KOIOS_SUBMIT_URL="${KOIOS_SUBMIT_URL:-https://preview.koios.rest/api/v1/submittx}"
OFFLINE_BUILD="${OFFLINE_BUILD:-0}"
TX_IN="${TX_IN:-}"
INPUT_LOVELACE="${INPUT_LOVELACE:-}"
PROTOCOL_PARAMS_FILE="${PROTOCOL_PARAMS_FILE:-}"
STAKE_KEY_DEPOSIT="${STAKE_KEY_DEPOSIT:-2000000}"
POOL_DEPOSIT="${POOL_DEPOSIT:-500000000}"
FEE_ITERATIONS="${FEE_ITERATIONS:-4}"
MIN_REQUIRED_LOVELACE="${MIN_REQUIRED_LOVELACE:-510000000}"

PAYMENT_ADDR_FILE="${PAYMENT_ADDR_FILE:-${REGISTRATION_DIR:+$REGISTRATION_DIR/payment.addr}}"
PAYMENT_SKEY="${PAYMENT_SKEY:-${REGISTRATION_DIR:+$REGISTRATION_DIR/payment.skey}}"
STAKE_SKEY="${STAKE_SKEY:-${REGISTRATION_DIR:+$REGISTRATION_DIR/stake.skey}}"
COLD_SKEY="${COLD_SKEY:-${CRED_DIR:+$CRED_DIR/cold.skey}}"
STAKE_REG_CERT="${STAKE_REG_CERT:-${REGISTRATION_DIR:+$REGISTRATION_DIR/stake.reg.cert}}"
POOL_REG_CERT="${POOL_REG_CERT:-${REGISTRATION_DIR:+$REGISTRATION_DIR/pool.reg.cert}}"
STAKE_DELEG_CERT="${STAKE_DELEG_CERT:-${REGISTRATION_DIR:+$REGISTRATION_DIR/stake.deleg.cert}}"
WORK_DIR="${WORK_DIR:-${REGISTRATION_DIR:+$REGISTRATION_DIR/tx-$(date -u +%Y%m%dT%H%M%SZ)}}"

usage() {
  cat <<'EOF'
Usage:
  CRED_DIR=/tmp/ygg-preview-generated-bp-... \
  SOCKET_PATH=/tmp/preview/node.socket \
  crates/node/yggdrasil-node/scripts/register_preview_generated_pool.sh

Offline public-submit mode used when a synced local socket is not available:
  CRED_DIR=/tmp/ygg-preview-generated-bp-... \
  OFFLINE_BUILD=1 \
  TX_IN=<funding-txid#ix> \
  INPUT_LOVELACE=<funding-lovelace> \
  PROTOCOL_PARAMS_FILE=/path/to/protocol-params.json \
  KOIOS_SUBMIT=1 \
  crates/node/yggdrasil-node/scripts/register_preview_generated_pool.sh

Builds and signs a preview pool registration transaction from generated
registration-support material:
  $CRED_DIR/registration/payment.addr
  $CRED_DIR/registration/payment.skey
  $CRED_DIR/registration/stake.skey
  $CRED_DIR/registration/stake.reg.cert
  $CRED_DIR/registration/pool.reg.cert
  $CRED_DIR/registration/stake.deleg.cert
  $CRED_DIR/cold.skey

The funding address must already have preview tADA. In online mode the script
queries the address UTxO when TX_IN is not supplied, picks the largest
lovelace-only input, builds a balanced Conway transaction, signs it with
payment/stake/cold keys, and writes a summary under WORK_DIR. In offline mode,
TX_IN, INPUT_LOVELACE, and PROTOCOL_PARAMS_FILE are required; the script builds
a raw transaction with iterative minimum-fee calculation.

Optional env:
  CARDANO_CLI           Default: vendored 11.0.1 cardano-cli if present, else cardano-cli
  REGISTRATION_DIR      Default: $CRED_DIR/registration
  NETWORK_MAGIC         Default: 2; this script is preview-only and rejects other values
  OFFLINE_BUILD         Default: 0. Set 1 to use build-raw without a local socket.
  TX_IN                 Optional explicit TxId#TxIx. If unset, query UTxO by payment address.
  INPUT_LOVELACE        Required with OFFLINE_BUILD=1.
  PROTOCOL_PARAMS_FILE  Required with OFFLINE_BUILD=1.
  STAKE_KEY_DEPOSIT     Default: 2000000.
  POOL_DEPOSIT          Default: 500000000.
  FEE_ITERATIONS        Default: 4.
  MIN_REQUIRED_LOVELACE Default: 510000000, checked only for auto-selected UTxO
  WORK_DIR              Default: $REGISTRATION_DIR/tx-<UTC timestamp>
  SUBMIT                Default: 0. Set 1 to submit through SOCKET_PATH after signing.
  KOIOS_SUBMIT          Default: 0. Set 1 to submit signed CBOR to KOIOS_SUBMIT_URL.
  KOIOS_SUBMIT_URL      Default: https://preview.koios.rest/api/v1/submittx

Exit codes:
  0   Transaction built/signed, and submitted when SUBMIT=1 or KOIOS_SUBMIT=1.
  1   Missing prerequisites or cardano-cli failure.
  2   Bad invocation.
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

require_preview() {
  if [[ "$NETWORK_MAGIC" != "2" ]]; then
    echo "ERROR: register_preview_generated_pool.sh is preview-only; NETWORK_MAGIC must be 2" >&2
    return 2
  fi
}

ensure_tools() {
  if ! command -v "$CARDANO_CLI" >/dev/null 2>&1; then
    echo "ERROR: cardano-cli not found (set CARDANO_CLI=/abs/path)" >&2
    return 1
  fi
  if ! command -v python3 >/dev/null 2>&1; then
    echo "ERROR: python3 is required to select a funding UTxO" >&2
    return 1
  fi
  if [[ "$KOIOS_SUBMIT" == "1" ]]; then
    if ! command -v curl >/dev/null 2>&1; then
      echo "ERROR: curl is required when KOIOS_SUBMIT=1" >&2
      return 1
    fi
    if ! command -v xxd >/dev/null 2>&1; then
      echo "ERROR: xxd is required when KOIOS_SUBMIT=1" >&2
      return 1
    fi
  fi
}

read_trimmed() {
  tr -d '\n\r' <"$1"
}

select_largest_utxo() {
  local utxo_json="$1"
  local min_lovelace="$2"
  python3 - "$utxo_json" "$min_lovelace" <<'PY'
import json
import sys

path = sys.argv[1]
minimum = int(sys.argv[2])
with open(path, "r", encoding="utf-8") as handle:
    utxos = json.load(handle)

best_txin = None
best_lovelace = -1
for txin, entry in utxos.items():
    value = entry.get("value", {})
    lovelace = value.get("lovelace")
    if isinstance(lovelace, int) and lovelace > best_lovelace:
        best_txin = txin
        best_lovelace = lovelace

if best_txin is None:
    print("ERROR: no lovelace UTxO found at payment address", file=sys.stderr)
    sys.exit(1)
if best_lovelace < minimum:
    print(
        f"ERROR: largest UTxO {best_txin} has {best_lovelace} lovelace, "
        f"below MIN_REQUIRED_LOVELACE={minimum}",
        file=sys.stderr,
    )
    sys.exit(1)

print(f"{best_txin}\t{best_lovelace}")
PY
}

write_summary() {
  local summary="$1"
  local payment_address="$2"
  local selected_lovelace="$3"
  local tx_body="$4"
  local tx_signed="$5"
  local submitted="$6"
  local tx_cbor_hex="$7"
  local tx_cbor="$8"
  local tx_id_file="$9"
  local koios_response="${10}"

  {
    echo "network_magic: $NETWORK_MAGIC"
    echo "offline_build: $OFFLINE_BUILD"
    echo "cred_dir: $CRED_DIR"
    echo "registration_dir: $REGISTRATION_DIR"
    echo "payment_address: $payment_address"
    echo "tx_in: $TX_IN"
    echo "selected_lovelace: ${selected_lovelace:-unknown}"
    echo "input_lovelace: ${INPUT_LOVELACE:-unknown}"
    echo "stake_key_deposit: $STAKE_KEY_DEPOSIT"
    echo "pool_deposit: $POOL_DEPOSIT"
    echo "certificate_order:"
    echo "  1. $STAKE_REG_CERT"
    echo "  2. $POOL_REG_CERT"
    echo "  3. $STAKE_DELEG_CERT"
    echo "tx_body: $tx_body"
    echo "tx_signed: $tx_signed"
    echo "tx_cbor_hex: $tx_cbor_hex"
    echo "tx_cbor: $tx_cbor"
    echo "tx_id_file: $tx_id_file"
    echo "koios_response: ${koios_response:-none}"
    echo "submitted: $submitted"
  } >"$summary"
}

extract_signed_tx_artifacts() {
  local tx_signed="$1"
  local tx_cbor_hex="$2"
  local tx_cbor="$3"
  local tx_id_file="$4"

  "$CARDANO_CLI" conway transaction txid --tx-file "$tx_signed" >"$tx_id_file"
  python3 - "$tx_signed" "$tx_cbor_hex" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as handle:
    data = json.load(handle)
hex_value = data.get("cborHex") or data.get("cbor_hex")
if not hex_value:
    print("ERROR: signed transaction does not contain cborHex", file=sys.stderr)
    sys.exit(1)
with open(sys.argv[2], "w", encoding="utf-8") as handle:
    handle.write(hex_value)
PY
  xxd -r -p "$tx_cbor_hex" >"$tx_cbor"
}

build_online_transaction() {
  local payment_address="$1"
  local tx_body="$2"

  # Certificate order is intentional: register stake key, register pool, then
  # delegate stake to the now-registered pool.
  "$CARDANO_CLI" conway transaction build \
    --testnet-magic "$NETWORK_MAGIC" \
    --socket-path "$SOCKET_PATH" \
    --tx-in "$TX_IN" \
    --change-address "$payment_address" \
    --certificate-file "$STAKE_REG_CERT" \
    --certificate-file "$POOL_REG_CERT" \
    --certificate-file "$STAKE_DELEG_CERT" \
    --witness-override 3 \
    --out-file "$tx_body"
}

require_positive_uint() {
  local name="$1"
  local value="$2"
  if [[ ! "$value" =~ ^[0-9]+$ || "$value" == "0" ]]; then
    echo "ERROR: $name must be a positive integer, got '$value'" >&2
    return 2
  fi
}

build_offline_transaction() {
  local payment_address="$1"
  local tx_body="$2"
  local fee=0
  local deposits output body calc new_fee i

  if [[ -z "$TX_IN" ]]; then
    echo "ERROR: OFFLINE_BUILD=1 requires TX_IN" >&2
    return 2
  fi
  require_positive_uint "INPUT_LOVELACE" "$INPUT_LOVELACE"
  require_positive_uint "STAKE_KEY_DEPOSIT" "$STAKE_KEY_DEPOSIT"
  require_positive_uint "POOL_DEPOSIT" "$POOL_DEPOSIT"
  require_positive_uint "FEE_ITERATIONS" "$FEE_ITERATIONS"
  require_file "$PROTOCOL_PARAMS_FILE" "PROTOCOL_PARAMS_FILE"

  deposits=$((STAKE_KEY_DEPOSIT + POOL_DEPOSIT))
  if (( INPUT_LOVELACE <= deposits )); then
    echo "ERROR: INPUT_LOVELACE must exceed deposits ($deposits)" >&2
    return 2
  fi

  for ((i = 1; i <= FEE_ITERATIONS; i++)); do
    output=$((INPUT_LOVELACE - deposits - fee))
    if (( output <= 0 )); then
      echo "ERROR: computed change output is non-positive ($output)" >&2
      return 2
    fi
    body="$WORK_DIR/txbody-iter${i}.json"
    "$CARDANO_CLI" conway transaction build-raw \
      --tx-in "$TX_IN" \
      --tx-out "$payment_address $output" \
      --fee "$fee" \
      --certificate-file "$STAKE_REG_CERT" \
      --certificate-file "$POOL_REG_CERT" \
      --certificate-file "$STAKE_DELEG_CERT" \
      --out-file "$body"
    calc="$("$CARDANO_CLI" conway transaction calculate-min-fee \
      --tx-body-file "$body" \
      --protocol-params-file "$PROTOCOL_PARAMS_FILE" \
      --witness-count 3 \
      --output-text)"
    new_fee="${calc%% *}"
    echo "[info] offline fee iteration=$i fee=$fee calculated=$new_fee output=$output"
    if [[ "$new_fee" == "$fee" ]]; then
      break
    fi
    fee="$new_fee"
  done

  output=$((INPUT_LOVELACE - deposits - fee))
  if (( output <= 0 )); then
    echo "ERROR: final change output is non-positive ($output)" >&2
    return 2
  fi
  "$CARDANO_CLI" conway transaction build-raw \
    --tx-in "$TX_IN" \
    --tx-out "$payment_address $output" \
    --fee "$fee" \
    --certificate-file "$STAKE_REG_CERT" \
    --certificate-file "$POOL_REG_CERT" \
    --certificate-file "$STAKE_DELEG_CERT" \
    --out-file "$tx_body"
}

submit_via_koios() {
  local tx_cbor="$1"
  local response_file="$2"
  local http_code

  http_code="$(curl -sS --max-time 60 \
    -o "$response_file" \
    -w '%{http_code}' \
    -H 'Content-Type: application/cbor' \
    --data-binary "@$tx_cbor" \
    "$KOIOS_SUBMIT_URL" || true)"
  echo "[info] Koios submit HTTP status: $http_code"
  if [[ "$http_code" != 2* ]]; then
    echo "ERROR: Koios submit failed; response at $response_file" >&2
    return 1
  fi
}

main() {
  if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
  fi

  require_preview
  require_bool01 "SUBMIT" "$SUBMIT"
  require_bool01 "KOIOS_SUBMIT" "$KOIOS_SUBMIT"
  require_bool01 "OFFLINE_BUILD" "$OFFLINE_BUILD"
  ensure_tools

  if [[ -z "$CRED_DIR" || ! -d "$CRED_DIR" ]]; then
    echo "ERROR: CRED_DIR must point at the generated preview credential directory" >&2
    exit 1
  fi
  if [[ -z "$REGISTRATION_DIR" || ! -d "$REGISTRATION_DIR" ]]; then
    echo "ERROR: REGISTRATION_DIR must point at generated registration-support material" >&2
    exit 1
  fi
  if [[ "$OFFLINE_BUILD" != "1" && ( -z "$SOCKET_PATH" || ! -S "$SOCKET_PATH" ) ]]; then
    echo "ERROR: SOCKET_PATH must be a running preview node Unix socket: $SOCKET_PATH" >&2
    exit 1
  fi
  if [[ "$SUBMIT" == "1" && ( -z "$SOCKET_PATH" || ! -S "$SOCKET_PATH" ) ]]; then
    echo "ERROR: SUBMIT=1 requires SOCKET_PATH to be a running preview node Unix socket: $SOCKET_PATH" >&2
    exit 1
  fi

  require_file "$PAYMENT_ADDR_FILE" "PAYMENT_ADDR_FILE"
  require_file "$PAYMENT_SKEY" "PAYMENT_SKEY"
  require_file "$STAKE_SKEY" "STAKE_SKEY"
  require_file "$COLD_SKEY" "COLD_SKEY"
  require_file "$STAKE_REG_CERT" "STAKE_REG_CERT"
  require_file "$POOL_REG_CERT" "POOL_REG_CERT"
  require_file "$STAKE_DELEG_CERT" "STAKE_DELEG_CERT"

  local payment_address utxo_json selection selected_lovelace tx_body tx_signed tx_cbor_hex tx_cbor tx_id_file summary submitted koios_response
  payment_address="$(read_trimmed "$PAYMENT_ADDR_FILE")"
  install -d -m 700 "$WORK_DIR"
  utxo_json="$WORK_DIR/payment-utxo.json"
  tx_body="$WORK_DIR/pool-registration.txbody"
  tx_signed="$WORK_DIR/pool-registration.signed"
  tx_cbor_hex="$WORK_DIR/pool-registration.cborhex"
  tx_cbor="$WORK_DIR/pool-registration.cbor"
  tx_id_file="$WORK_DIR/pool-registration.txid"
  summary="$WORK_DIR/registration-summary.txt"
  koios_response="$WORK_DIR/koios-submit-response.txt"
  selected_lovelace=""

  if [[ "$OFFLINE_BUILD" != "1" && -z "$TX_IN" ]]; then
    "$CARDANO_CLI" conway query utxo \
      --testnet-magic "$NETWORK_MAGIC" \
      --socket-path "$SOCKET_PATH" \
      --address "$payment_address" \
      --output-json \
      --out-file "$utxo_json"
    selection="$(select_largest_utxo "$utxo_json" "$MIN_REQUIRED_LOVELACE")"
    TX_IN="${selection%%$'\t'*}"
    selected_lovelace="${selection#*$'\t'}"
    echo "[info] selected funding UTxO: $TX_IN ($selected_lovelace lovelace)"
  else
    echo "[info] using explicit funding UTxO: $TX_IN"
  fi

  if [[ "$OFFLINE_BUILD" == "1" ]]; then
    build_offline_transaction "$payment_address" "$tx_body"
  else
    build_online_transaction "$payment_address" "$tx_body"
  fi

  "$CARDANO_CLI" conway transaction sign \
    --tx-body-file "$tx_body" \
    --testnet-magic "$NETWORK_MAGIC" \
    --signing-key-file "$PAYMENT_SKEY" \
    --signing-key-file "$STAKE_SKEY" \
    --signing-key-file "$COLD_SKEY" \
    --out-file "$tx_signed"
  extract_signed_tx_artifacts "$tx_signed" "$tx_cbor_hex" "$tx_cbor" "$tx_id_file"

  submitted="no"
  if [[ "$SUBMIT" == "1" ]]; then
    "$CARDANO_CLI" conway transaction submit \
      --testnet-magic "$NETWORK_MAGIC" \
      --socket-path "$SOCKET_PATH" \
      --tx-file "$tx_signed"
    submitted="node"
  fi
  if [[ "$KOIOS_SUBMIT" == "1" ]]; then
    submit_via_koios "$tx_cbor" "$koios_response"
    submitted="koios"
  fi

  write_summary "$summary" "$payment_address" "$selected_lovelace" "$tx_body" "$tx_signed" "$submitted" "$tx_cbor_hex" "$tx_cbor" "$tx_id_file" "${koios_response:-}"

  echo "[ok] tx body:  $tx_body"
  echo "[ok] signed:   $tx_signed"
  echo "[ok] tx id:    $tx_id_file"
  echo "[ok] cbor hex: $tx_cbor_hex"
  echo "[ok] summary:  $summary"
  if [[ "$SUBMIT" != "1" && "$KOIOS_SUBMIT" != "1" ]]; then
    echo "[info] not submitted; rerun with SUBMIT=1 or KOIOS_SUBMIT=1 after reviewing the signed transaction"
  fi
}

main "$@"
