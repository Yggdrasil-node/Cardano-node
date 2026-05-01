#!/usr/bin/env bash
set -euo pipefail

# Generate and smoke-test a preview-only Yggdrasil producer bundle.
#
# This uses upstream cardano-cli text-envelope commands for the actual key
# material and writes all generated files under OUT_DIR, which defaults to the
# repository's ignored tmp/ tree.  The generated pool is not registered on-chain
# until the payment address is funded and the emitted certificates are submitted;
# before that the producer runtime can start, validate credentials, sync, and
# run the forge loop, but it will not be elected.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PREVIEW_REF_DIR="$ROOT_DIR/node/configuration/preview"
OUT_DIR="${OUT_DIR:-$ROOT_DIR/tmp/preview-producer}"
KEY_DIR="$OUT_DIR/keys"
CONFIG_DIR="$OUT_DIR/config"
RUN_DIR="$OUT_DIR/run"
LOG_DIR="${LOG_DIR:-$OUT_DIR/logs}"
WALLET_DIR="${WALLET_DIR:-$OUT_DIR/wallet}"
CERT_DIR="${CERT_DIR:-$OUT_DIR/certs}"
METADATA_DIR="${METADATA_DIR:-$OUT_DIR/metadata}"

CARDANO_CLI="${CARDANO_CLI:-cardano-cli}"
YGG_BIN="${YGG_BIN:-$ROOT_DIR/target/release/yggdrasil-node}"
RUN_SECONDS="${RUN_SECONDS:-60}"
FORCE="${FORCE:-0}"

PREVIEW_BOOTSTRAP_HOST="${PREVIEW_BOOTSTRAP_HOST:-preview-node.play.dev.cardano.org}"
PREVIEW_BOOTSTRAP_PORT="${PREVIEW_BOOTSTRAP_PORT:-3001}"
RELAY_LISTEN_HOST="${RELAY_LISTEN_HOST:-127.0.0.1}"
RELAY_LISTEN_PORT="${RELAY_LISTEN_PORT:-13001}"
RELAY_METRICS_PORT="${RELAY_METRICS_PORT:-19001}"
PRODUCER_METRICS_PORT="${PRODUCER_METRICS_PORT:-19002}"
POOL_PLEDGE="${POOL_PLEDGE:-0}"
POOL_MARGIN="${POOL_MARGIN:-0/1}"
POOL_RELAY_PORT="${POOL_RELAY_PORT:-3001}"
POOL_TICKER="${POOL_TICKER:-RUST}"
POOL_NAME="${POOL_NAME:-WORLDS FIRST RUST NODE}"
POOL_DESCRIPTION="${POOL_DESCRIPTION:-Yggdrasil preview stake pool operated by the pure Rust Cardano node implementation.}"
POOL_HOMEPAGE="${POOL_HOMEPAGE:-https://github.com/Yggdrasil-node/Cardano-node}"
POOL_METADATA_URL="${POOL_METADATA_URL:-https://yggdrasil-node.github.io/Cardano-node/poolMetaData.json}"

usage() {
  cat <<'EOF'
Usage:
  node/scripts/preview_producer_harness.sh generate
  node/scripts/preview_producer_harness.sh wallet
  node/scripts/preview_producer_harness.sh certs
  node/scripts/preview_producer_harness.sh funding-address
  node/scripts/preview_producer_harness.sh validate
  node/scripts/preview_producer_harness.sh smoke-relay
  node/scripts/preview_producer_harness.sh smoke-producer
  node/scripts/preview_producer_harness.sh endurance-relay
  node/scripts/preview_producer_harness.sh endurance-producer
  node/scripts/preview_producer_harness.sh all

Commands:
  generate        Generate preview KES/VRF/cold/OpCert files and configs.
  wallet          Generate preview payment/stake keys and addresses.
  certs           Generate preview stake registration, delegation, and pool certs.
  funding-address Print the preview payment address to fund.
  validate        Run yggdrasil validate-config for generated relay and producer configs.
  smoke-relay     Start a bounded preview relay run and require bootstrap + metrics.
  smoke-producer  Start a bounded preview producer-mode run with generated credentials.
  endurance-relay Run relay for the full RUN_SECONDS and require slot progress.
  endurance-producer
                 Run producer for the full RUN_SECONDS and require slot progress.
  all             generate + wallet + certs + validate + smoke-relay + smoke-producer.

Environment:
  OUT_DIR                 Default: ./tmp/preview-producer
  CARDANO_CLI            Default: cardano-cli
  YGG_BIN                Default: target/release/yggdrasil-node
  RUN_SECONDS            Default: 60
  MIN_SLOT_ADVANCE       Default: 1000 for endurance-* commands; set 0 to disable.
  FORCE=1                Replace an existing generated bundle.
  KES_PERIOD=<n>         Override derived preview KES period.
  CURRENT_SLOT=<n>       Override current slot used to derive KES period.
  RELAY_LISTEN_HOST      Default: 127.0.0.1
  RELAY_LISTEN_PORT      Default: 13001
  RELAY_METRICS_PORT     Default: 19001
  PRODUCER_METRICS_PORT  Default: 19002
  POOL_PLEDGE            Default: 0 lovelace
  POOL_MARGIN            Default: 0/1
  POOL_TICKER            Default: RUST
  POOL_NAME              Default: WORLDS FIRST RUST NODE
  POOL_DESCRIPTION       Default: Yggdrasil preview stake pool description.
  POOL_HOMEPAGE          Default: https://github.com/Yggdrasil-node/Cardano-node
  POOL_METADATA_URL      Default: GitHub Pages poolMetaData.json URL.
  POOL_RELAY_DNS         Optional relay DNS name to publish in the pool cert.
  POOL_RELAY_IPV4        Optional relay IPv4 address to publish in the pool cert.
  POOL_RELAY_PORT        Default: 3001 when a relay is published.

Notes:
  The generated cold key is not registered as a preview stake pool. This harness
  proves key/config generation, credential validation, runtime startup, sync,
  and forge-loop activation. Actual block adoption requires preview tADA, pool
  registration, delegation, and enough active stake to win leader slots.
EOF
}

require_tool() {
  local tool="$1"
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "ERROR: required tool not found in PATH: $tool" >&2
    exit 1
  fi
}

require_file() {
  local path="$1"
  local name="$2"
  if [[ ! -f "$path" ]]; then
    echo "ERROR: missing $name: $path" >&2
    exit 1
  fi
}

require_prereqs() {
  require_tool "$CARDANO_CLI"
  require_tool jq
  require_tool date
  require_tool awk
  require_tool grep
  require_tool curl
  require_tool getent
  require_tool rg
  require_file "$PREVIEW_REF_DIR/config.json" "preview config.json"
  require_file "$PREVIEW_REF_DIR/topology.json" "preview topology.json"
  require_file "$PREVIEW_REF_DIR/shelley-genesis.json" "preview shelley-genesis.json"
}

resolve_preview_peer_addr() {
  if [[ -n "${PREVIEW_PEER_ADDR:-}" ]]; then
    printf '%s\n' "$PREVIEW_PEER_ADDR"
    return
  fi

  local ip
  ip="$(getent ahostsv4 "$PREVIEW_BOOTSTRAP_HOST" 2>/dev/null | awk '{ print $1; exit }' || true)"
  if [[ -z "$ip" ]]; then
    ip="$(getent hosts "$PREVIEW_BOOTSTRAP_HOST" 2>/dev/null | awk '{ print $1; exit }' || true)"
  fi
  if [[ -z "$ip" ]]; then
    echo "ERROR: failed to resolve $PREVIEW_BOOTSTRAP_HOST; set PREVIEW_PEER_ADDR=ip:port" >&2
    exit 1
  fi
  printf '%s:%s\n' "$ip" "$PREVIEW_BOOTSTRAP_PORT"
}

preview_current_slot() {
  if [[ -n "${CURRENT_SLOT:-}" ]]; then
    printf '%s\n' "$CURRENT_SLOT"
    return
  fi

  local system_start slot_length now start
  system_start="$(jq -r '.systemStart' "$PREVIEW_REF_DIR/shelley-genesis.json")"
  slot_length="$(jq -r '.slotLength | floor' "$PREVIEW_REF_DIR/shelley-genesis.json")"
  start="$(date -u -d "$system_start" +%s)"
  now="$(date -u +%s)"
  if [[ "$slot_length" -le 0 ]]; then
    echo "ERROR: invalid preview slotLength: $slot_length" >&2
    exit 1
  fi
  printf '%s\n' "$(((now - start) / slot_length))"
}

preview_kes_period() {
  if [[ -n "${KES_PERIOD:-}" ]]; then
    printf '%s\n' "$KES_PERIOD"
    return
  fi

  local slot slots_per_kes
  slot="$(preview_current_slot)"
  slots_per_kes="$(jq -r '.slotsPerKESPeriod' "$PREVIEW_REF_DIR/shelley-genesis.json")"
  if [[ "$slots_per_kes" -le 0 ]]; then
    echo "ERROR: invalid preview slotsPerKESPeriod: $slots_per_kes" >&2
    exit 1
  fi
  printf '%s\n' "$((slot / slots_per_kes))"
}

prepare_output_dirs() {
  if [[ -e "$OUT_DIR" && "$FORCE" != "1" ]]; then
    echo "ERROR: OUT_DIR already exists: $OUT_DIR" >&2
    echo "Set FORCE=1 to replace this generated preview bundle." >&2
    exit 1
  fi
  if [[ "$FORCE" == "1" ]]; then
    rm -rf "$OUT_DIR"
  fi
  mkdir -p "$KEY_DIR" "$CONFIG_DIR" "$RUN_DIR" "$LOG_DIR"
  chmod 0700 "$OUT_DIR" "$KEY_DIR" "$RUN_DIR" "$LOG_DIR"
}

copy_preview_reference_files() {
  cp "$PREVIEW_REF_DIR/byron-genesis.json" "$CONFIG_DIR/"
  cp "$PREVIEW_REF_DIR/shelley-genesis.json" "$CONFIG_DIR/"
  cp "$PREVIEW_REF_DIR/alonzo-genesis.json" "$CONFIG_DIR/"
  cp "$PREVIEW_REF_DIR/conway-genesis.json" "$CONFIG_DIR/"
  cp "$PREVIEW_REF_DIR/topology.json" "$CONFIG_DIR/"
  cp "$PREVIEW_REF_DIR/peer-snapshot.json" "$CONFIG_DIR/"
  if [[ -f "$PREVIEW_REF_DIR/checkpoints.json" ]]; then
    cp "$PREVIEW_REF_DIR/checkpoints.json" "$CONFIG_DIR/"
  fi
}

generate_keys() {
  local kes_period="$1"

  "$CARDANO_CLI" node key-gen \
    --cold-verification-key-file "$KEY_DIR/cold.vkey" \
    --cold-signing-key-file "$KEY_DIR/cold.skey" \
    --operational-certificate-issue-counter-file "$KEY_DIR/cold.counter"

  "$CARDANO_CLI" node key-gen-VRF \
    --verification-key-file "$KEY_DIR/vrf.vkey" \
    --signing-key-file "$KEY_DIR/vrf.skey"

  "$CARDANO_CLI" node key-gen-KES \
    --verification-key-file "$KEY_DIR/kes.vkey" \
    --signing-key-file "$KEY_DIR/kes.skey"

  "$CARDANO_CLI" node issue-op-cert \
    --kes-verification-key-file "$KEY_DIR/kes.vkey" \
    --cold-signing-key-file "$KEY_DIR/cold.skey" \
    --operational-certificate-issue-counter-file "$KEY_DIR/cold.counter" \
    --kes-period "$kes_period" \
    --out-file "$KEY_DIR/node.opcert"

  chmod 0400 "$KEY_DIR/cold.skey" "$KEY_DIR/vrf.skey" "$KEY_DIR/kes.skey"
  chmod 0600 "$KEY_DIR/cold.counter"
  chmod 0644 "$KEY_DIR/cold.vkey" "$KEY_DIR/vrf.vkey" "$KEY_DIR/kes.vkey" "$KEY_DIR/node.opcert"
}

ensure_generated_bundle() {
  require_file "$KEY_DIR/cold.vkey" "generated cold verification key"
  require_file "$KEY_DIR/vrf.vkey" "generated VRF verification key"
}

ensure_wallet() {
  require_file "$WALLET_DIR/payment.vkey" "preview payment verification key"
  require_file "$WALLET_DIR/payment.skey" "preview payment signing key"
  require_file "$WALLET_DIR/stake.vkey" "preview stake verification key"
  require_file "$WALLET_DIR/stake.skey" "preview stake signing key"
  require_file "$WALLET_DIR/payment.addr" "preview payment address"
  require_file "$WALLET_DIR/stake.addr" "preview stake address"
}

generate_wallet() {
  require_prereqs

  if [[ -e "$WALLET_DIR" && "$FORCE" == "1" ]]; then
    rm -rf "$WALLET_DIR"
  fi
  mkdir -p "$WALLET_DIR"
  chmod 0700 "$WALLET_DIR"

  if [[ -f "$WALLET_DIR/payment.addr" && "$FORCE" != "1" ]]; then
    ensure_wallet
  else
    echo "[info] generating preview funding wallet in $WALLET_DIR"
    "$CARDANO_CLI" latest address key-gen \
      --verification-key-file "$WALLET_DIR/payment.vkey" \
      --signing-key-file "$WALLET_DIR/payment.skey"

    "$CARDANO_CLI" latest stake-address key-gen \
      --verification-key-file "$WALLET_DIR/stake.vkey" \
      --signing-key-file "$WALLET_DIR/stake.skey"

    "$CARDANO_CLI" latest address build \
      --payment-verification-key-file "$WALLET_DIR/payment.vkey" \
      --stake-verification-key-file "$WALLET_DIR/stake.vkey" \
      --testnet-magic 2 \
      --out-file "$WALLET_DIR/payment.addr"

    "$CARDANO_CLI" latest stake-address build \
      --stake-verification-key-file "$WALLET_DIR/stake.vkey" \
      --testnet-magic 2 \
      --out-file "$WALLET_DIR/stake.addr"

    chmod 0400 "$WALLET_DIR/payment.skey" "$WALLET_DIR/stake.skey"
    chmod 0644 "$WALLET_DIR/payment.vkey" "$WALLET_DIR/stake.vkey" "$WALLET_DIR/payment.addr" "$WALLET_DIR/stake.addr"
  fi

  echo "[ok] preview payment address:"
  cat "$WALLET_DIR/payment.addr"
  echo
  echo "[ok] preview stake address:"
  cat "$WALLET_DIR/stake.addr"
  echo
}

pool_relay_args() {
  local -n out_args="$1"
  out_args=()
  if [[ -n "${POOL_RELAY_DNS:-}" ]]; then
    out_args+=(--single-host-pool-relay "$POOL_RELAY_DNS" --pool-relay-port "$POOL_RELAY_PORT")
  elif [[ -n "${POOL_RELAY_IPV4:-}" ]]; then
    out_args+=(--pool-relay-ipv4 "$POOL_RELAY_IPV4" --pool-relay-port "$POOL_RELAY_PORT")
  fi
}

write_pool_metadata() {
  mkdir -p "$METADATA_DIR"
  chmod 0755 "$METADATA_DIR"

  jq -n \
    --arg name "$POOL_NAME" \
    --arg ticker "$POOL_TICKER" \
    --arg description "$POOL_DESCRIPTION" \
    --arg homepage "$POOL_HOMEPAGE" \
    '{
      name: $name,
      description: $description,
      ticker: $ticker,
      homepage: $homepage
    }' >"$METADATA_DIR/poolMetaData.json"

  "$CARDANO_CLI" latest stake-pool metadata-hash \
    --pool-metadata-file "$METADATA_DIR/poolMetaData.json" \
    --out-file "$METADATA_DIR/poolMetaData.hash"
}

generate_registration_certs() {
  require_prereqs
  ensure_generated_bundle
  if [[ ! -f "$WALLET_DIR/payment.addr" ]]; then
    generate_wallet
  fi
  ensure_wallet

  if [[ -e "$CERT_DIR" && "$FORCE" == "1" ]]; then
    rm -rf "$CERT_DIR"
  fi
  mkdir -p "$CERT_DIR"
  chmod 0755 "$CERT_DIR"

  local key_deposit pool_cost pool_deposit
  key_deposit="$(jq -r '.protocolParams.keyDeposit' "$PREVIEW_REF_DIR/shelley-genesis.json")"
  pool_deposit="$(jq -r '.protocolParams.poolDeposit' "$PREVIEW_REF_DIR/shelley-genesis.json")"
  pool_cost="$(jq -r '.protocolParams.minPoolCost' "$PREVIEW_REF_DIR/shelley-genesis.json")"

  local relay_args=()
  pool_relay_args relay_args
  write_pool_metadata
  local metadata_hash
  metadata_hash="$(cat "$METADATA_DIR/poolMetaData.hash")"

  echo "[info] writing preview registration certificates in $CERT_DIR"
  "$CARDANO_CLI" latest stake-address registration-certificate \
    --stake-verification-key-file "$WALLET_DIR/stake.vkey" \
    --key-reg-deposit-amt "$key_deposit" \
    --out-file "$CERT_DIR/stake-registration.cert"

  "$CARDANO_CLI" latest stake-address stake-delegation-certificate \
    --stake-verification-key-file "$WALLET_DIR/stake.vkey" \
    --cold-verification-key-file "$KEY_DIR/cold.vkey" \
    --out-file "$CERT_DIR/stake-delegation.cert"

  "$CARDANO_CLI" latest stake-pool registration-certificate \
    --cold-verification-key-file "$KEY_DIR/cold.vkey" \
    --vrf-verification-key-file "$KEY_DIR/vrf.vkey" \
    --pool-pledge "$POOL_PLEDGE" \
    --pool-cost "$pool_cost" \
    --pool-margin "$POOL_MARGIN" \
    --pool-reward-account-verification-key-file "$WALLET_DIR/stake.vkey" \
    --pool-owner-stake-verification-key-file "$WALLET_DIR/stake.vkey" \
    "${relay_args[@]}" \
    --metadata-url "$POOL_METADATA_URL" \
    --metadata-hash "$metadata_hash" \
    --testnet-magic 2 \
    --out-file "$CERT_DIR/pool-registration.cert"

  "$CARDANO_CLI" latest stake-pool id \
    --cold-verification-key-file "$KEY_DIR/cold.vkey" \
    --output-bech32 \
    --out-file "$CERT_DIR/pool.id"

  "$CARDANO_CLI" latest stake-pool id \
    --cold-verification-key-file "$KEY_DIR/cold.vkey" \
    --output-hex \
    --out-file "$CERT_DIR/pool.id.hex"

  cat >"$CERT_DIR/registration-summary.json" <<EOF
{
  "network": "preview",
  "network_magic": 2,
  "payment_address": "$(cat "$WALLET_DIR/payment.addr")",
  "stake_address": "$(cat "$WALLET_DIR/stake.addr")",
  "pool_id_bech32": "$(cat "$CERT_DIR/pool.id")",
  "pool_id_hex": "$(cat "$CERT_DIR/pool.id.hex")",
  "stake_key_deposit_lovelace": $key_deposit,
  "pool_deposit_lovelace": $pool_deposit,
  "pool_pledge_lovelace": $POOL_PLEDGE,
  "pool_cost_lovelace": $pool_cost,
  "pool_margin": "$POOL_MARGIN",
  "pool_metadata_file": "$METADATA_DIR/poolMetaData.json",
  "pool_metadata_url": "$POOL_METADATA_URL",
  "pool_metadata_hash": "$metadata_hash",
  "pool_ticker": "$POOL_TICKER",
  "pool_name": "$POOL_NAME"
}
EOF

  echo "[ok] generated preview registration material"
  jq . "$CERT_DIR/registration-summary.json"
}

write_config() {
  local role="$1"
  local config_path="$2"
  local storage_dir="$3"
  local socket_path="$4"
  local trace_name="$5"
  local peer_addr="$6"

  local byron_hash shelley_hash alonzo_hash conway_hash
  byron_hash="$(jq -r '.ByronGenesisHash' "$PREVIEW_REF_DIR/config.json")"
  shelley_hash="$(jq -r '.ShelleyGenesisHash' "$PREVIEW_REF_DIR/config.json")"
  alonzo_hash="$(jq -r '.AlonzoGenesisHash' "$PREVIEW_REF_DIR/config.json")"
  conway_hash="$(jq -r '.ConwayGenesisHash' "$PREVIEW_REF_DIR/config.json")"

  mkdir -p "$storage_dir"

  if [[ "$role" == "producer" ]]; then
    jq -n \
      --arg peer "$peer_addr" \
      --arg storage "$storage_dir" \
      --arg socket "$socket_path" \
      --arg trace_name "$trace_name" \
      --arg byron_hash "$byron_hash" \
      --arg shelley_hash "$shelley_hash" \
      --arg alonzo_hash "$alonzo_hash" \
      --arg conway_hash "$conway_hash" \
      --arg kes "$KEY_DIR/kes.skey" \
      --arg vrf "$KEY_DIR/vrf.skey" \
      --arg opcert "$KEY_DIR/node.opcert" \
      --arg issuer "$KEY_DIR/cold.vkey" \
      '{
        peer_addr: $peer,
        storage_dir: $storage,
        network_magic: 2,
        RequiresNetworkMagic: "RequiresMagic",
        Protocol: "Cardano",
        ConsensusMode: "GenesisMode",
        protocol_versions: [13, 14],
        slots_per_kes_period: 129600,
        max_kes_evolutions: 62,
        epoch_length: 86400,
        security_param_k: 432,
        active_slot_coeff: 0.05,
        max_major_protocol_version: 10,
        keepalive_interval_secs: 30,
        peer_sharing: 1,
        TopologyFilePath: "topology.json",
        ByronGenesisFile: "byron-genesis.json",
        ByronGenesisHash: $byron_hash,
        ShelleyGenesisFile: "shelley-genesis.json",
        ShelleyGenesisHash: $shelley_hash,
        AlonzoGenesisFile: "alonzo-genesis.json",
        AlonzoGenesisHash: $alonzo_hash,
        ConwayGenesisFile: "conway-genesis.json",
        ConwayGenesisHash: $conway_hash,
        SocketPath: $socket,
        TraceOptionNodeName: $trace_name,
        TurnOnLogging: true,
        UseTraceDispatcher: true,
        TurnOnLogMetrics: true,
        ShelleyKesKey: $kes,
        ShelleyVrfKey: $vrf,
        ShelleyOperationalCertificate: $opcert,
        ShelleyOperationalCertificateIssuerVkey: $issuer
      }' >"$config_path"
  else
    jq -n \
      --arg peer "$peer_addr" \
      --arg listen "$RELAY_LISTEN_HOST:$RELAY_LISTEN_PORT" \
      --arg storage "$storage_dir" \
      --arg socket "$socket_path" \
      --arg trace_name "$trace_name" \
      --arg byron_hash "$byron_hash" \
      --arg shelley_hash "$shelley_hash" \
      --arg alonzo_hash "$alonzo_hash" \
      --arg conway_hash "$conway_hash" \
      '{
        peer_addr: $peer,
        inbound_listen_addr: $listen,
        storage_dir: $storage,
        network_magic: 2,
        RequiresNetworkMagic: "RequiresMagic",
        Protocol: "Cardano",
        ConsensusMode: "GenesisMode",
        protocol_versions: [13, 14],
        slots_per_kes_period: 129600,
        max_kes_evolutions: 62,
        epoch_length: 86400,
        security_param_k: 432,
        active_slot_coeff: 0.05,
        max_major_protocol_version: 10,
        keepalive_interval_secs: 30,
        peer_sharing: 1,
        TopologyFilePath: "topology.json",
        ByronGenesisFile: "byron-genesis.json",
        ByronGenesisHash: $byron_hash,
        ShelleyGenesisFile: "shelley-genesis.json",
        ShelleyGenesisHash: $shelley_hash,
        AlonzoGenesisFile: "alonzo-genesis.json",
        AlonzoGenesisHash: $alonzo_hash,
        ConwayGenesisFile: "conway-genesis.json",
        ConwayGenesisHash: $conway_hash,
        SocketPath: $socket,
        TraceOptionNodeName: $trace_name,
        TurnOnLogging: true,
        UseTraceDispatcher: true,
        TurnOnLogMetrics: true
      }' >"$config_path"
  fi
}

write_runner() {
  local path="$1"
  local config_path="$2"
  local metrics_port="$3"
  local extra_flag="${4:-}"

  cat >"$path" <<EOF
#!/usr/bin/env bash
set -euo pipefail
YGG_BIN="\${YGG_BIN:-$YGG_BIN}"
exec "\$YGG_BIN" run --config "$config_path" --metrics-port "$metrics_port" $extra_flag "\$@"
EOF
  chmod 0755 "$path"
}

write_readme() {
  local slot="$1"
  local kes_period="$2"
  local peer_addr="$3"

  cat >"$OUT_DIR/README.md" <<EOF
# Yggdrasil Preview Producer Bundle

Generated: $(date -u +"%Y-%m-%dT%H:%M:%SZ")

Network: preview
Network magic: 2
Bootstrap peer: $peer_addr
Derived current slot: $slot
OpCert KES period: $kes_period

## Files

- keys/cold.skey, keys/cold.vkey, keys/cold.counter
- keys/vrf.skey, keys/vrf.vkey
- keys/kes.skey, keys/kes.vkey
- keys/node.opcert
- config/preview-relay.json
- config/preview-producer.json
- run/run-preview-relay.sh
- run/run-preview-producer.sh
- wallet/payment.addr
- wallet/stake.addr
- metadata/poolMetaData.json
- metadata/poolMetaData.hash
- certs/stake-registration.cert
- certs/stake-delegation.cert
- certs/pool-registration.cert
- certs/pool.id

## Validate

\`\`\`bash
$YGG_BIN validate-config --config "$CONFIG_DIR/preview-relay.json" --non-producing-node
$YGG_BIN validate-config --config "$CONFIG_DIR/preview-producer.json"
\`\`\`

## Smoke Runs

\`\`\`bash
RUN_SECONDS=60 node/scripts/preview_producer_harness.sh smoke-relay
RUN_SECONDS=60 node/scripts/preview_producer_harness.sh smoke-producer
\`\`\`

The generated cold key is not registered as a preview stake pool. The producer
smoke checks credential loading, OpCert validation, bootstrap connection,
metrics, sync, and forge-loop startup. Actual block adoption requires preview
tADA, pool registration, delegation, and enough active stake to win slots.
EOF
}

generate_bundle() {
  require_prereqs
  prepare_output_dirs
  copy_preview_reference_files

  local slot kes_period peer_addr
  slot="$(preview_current_slot)"
  kes_period="$(preview_kes_period)"
  peer_addr="$(resolve_preview_peer_addr)"

  echo "[info] generating preview keys in $KEY_DIR"
  echo "[info] current preview slot=$slot kesPeriod=$kes_period"
  generate_keys "$kes_period"

  write_config "relay" "$CONFIG_DIR/preview-relay.json" "$OUT_DIR/db/relay" "$RUN_DIR/preview-relay.sock" "yggdrasil-preview-relay" "$peer_addr"
  write_config "producer" "$CONFIG_DIR/preview-producer.json" "$OUT_DIR/db/producer" "$RUN_DIR/preview-producer.sock" "yggdrasil-preview-producer" "$peer_addr"
  write_runner "$RUN_DIR/run-preview-relay.sh" "$CONFIG_DIR/preview-relay.json" "$RELAY_METRICS_PORT" "--non-producing-node"
  write_runner "$RUN_DIR/run-preview-producer.sh" "$CONFIG_DIR/preview-producer.json" "$PRODUCER_METRICS_PORT"
  write_readme "$slot" "$kes_period" "$peer_addr"

  echo "[ok] generated preview bundle: $OUT_DIR"
}

validate_bundle() {
  require_prereqs
  require_file "$YGG_BIN" "yggdrasil-node binary"
  require_file "$CONFIG_DIR/preview-relay.json" "generated relay config"
  require_file "$CONFIG_DIR/preview-producer.json" "generated producer config"

  echo "[info] validating relay config"
  "$YGG_BIN" validate-config --config "$CONFIG_DIR/preview-relay.json" --non-producing-node >/tmp/ygg-preview-relay-validate.$$.json
  jq '{node_role, network_magic, storage_dir, resolved_startup_peer_count, warnings}' /tmp/ygg-preview-relay-validate.$$.json
  rm -f /tmp/ygg-preview-relay-validate.$$.json

  echo "[info] validating producer config"
  "$YGG_BIN" validate-config --config "$CONFIG_DIR/preview-producer.json" >/tmp/ygg-preview-producer-validate.$$.json
  jq '{node_role, network_magic, storage_dir, resolved_startup_peer_count, warnings}' /tmp/ygg-preview-producer-validate.$$.json
  rm -f /tmp/ygg-preview-producer-validate.$$.json

  echo "[ok] generated preview configs validated"
}

smoke_run() {
  local role="$1"
  local config_path="$2"
  local metrics_port="$3"
  local log_file="$LOG_DIR/preview-$role-$(date +%Y%m%d-%H%M%S).log"
  local metrics_file="$LOG_DIR/preview-$role-metrics.txt"

  require_prereqs
  require_file "$YGG_BIN" "yggdrasil-node binary"
  require_file "$config_path" "generated $role config"

  echo "[info] starting preview $role smoke for ${RUN_SECONDS}s"
  echo "[info] log: $log_file"

  local extra_args=()
  if [[ "$role" == "relay" ]]; then
    extra_args=(--non-producing-node)
  fi

  set +e
  "$YGG_BIN" run --config "$config_path" --metrics-port "$metrics_port" "${extra_args[@]}" >"$log_file" 2>&1 &
  local pid=$!
  set -e

  cleanup() {
    kill "$pid" >/dev/null 2>&1 || true
    wait "$pid" >/dev/null 2>&1 || true
  }
  trap cleanup EXIT INT TERM

  local elapsed connected metrics progress producer_started
  elapsed=0
  connected=0
  metrics=0
  progress=0
  producer_started=0

  while [[ "$elapsed" -lt "$RUN_SECONDS" ]]; do
    if ! kill -0 "$pid" >/dev/null 2>&1; then
      echo "ERROR: yggdrasil-node exited early" >&2
      tail -n 80 "$log_file" >&2 || true
      exit 1
    fi
    if rg -q "bootstrap peer connected|verified sync session established" "$log_file" 2>/dev/null; then
      connected=1
    fi
    if [[ "$role" != "producer" || "$producer_started" -eq 0 ]]; then
      if rg -q "Startup.BlockProducer|block producer loop started" "$log_file" 2>/dev/null; then
        producer_started=1
      fi
    fi
    if curl -fsS "http://127.0.0.1:$metrics_port/metrics" >"$metrics_file" 2>/dev/null; then
      rg -q '^yggdrasil_' "$metrics_file" && metrics=1
      if awk '/^yggdrasil_blocks_synced / { if ($2 + 0 > 0) found=1 } /^yggdrasil_current_slot / { if ($2 + 0 > 0) found=1 } END { exit found ? 0 : 1 }' "$metrics_file"; then
        progress=1
      fi
    fi
    if [[ "$connected" -eq 1 && "$metrics" -eq 1 && "$progress" -eq 1 ]]; then
      if [[ "$role" != "producer" || "$producer_started" -eq 1 ]]; then
        break
      fi
    fi
    sleep 1
    elapsed=$((elapsed + 1))
  done

  kill "$pid" >/dev/null 2>&1 || true
  wait "$pid" >/dev/null 2>&1 || true
  trap - EXIT INT TERM

  if grep -q "invalid VRF proof" "$log_file"; then
    echo "ERROR: observed invalid VRF proof" >&2
    exit 1
  fi
  if [[ "$connected" -ne 1 || "$metrics" -ne 1 || "$progress" -ne 1 ]]; then
    echo "ERROR: preview $role smoke did not reach required gates" >&2
    echo "[info] connected=$connected metrics=$metrics progress=$progress producer_started=$producer_started" >&2
    tail -n 100 "$log_file" >&2 || true
    exit 1
  fi
  if [[ "$role" == "producer" && "$producer_started" -ne 1 ]]; then
    echo "ERROR: producer smoke did not observe block producer startup" >&2
    tail -n 100 "$log_file" >&2 || true
    exit 1
  fi

  echo "[ok] preview $role smoke passed"
  echo "[info] log: $log_file"
  echo "[info] metrics:"
  rg '^(yggdrasil_current_slot|yggdrasil_blocks_synced|yggdrasil_active_peers|yggdrasil_known_peers|yggdrasil_blocks_forged|yggdrasil_blocks_adopted)' "$metrics_file" || true
  echo "[info] log evidence:"
  rg -i "Startup.BlockProducer|block producer loop started|bootstrap peer connected|sync complete|forged|leader|adopted|not leader|invalid VRF|error" "$log_file" | tail -n 80 || true
}

metric_value() {
  local metrics_file="$1"
  local metric="$2"
  awk -v metric="$metric" '$1 == metric { print int($2); found=1 } END { if (!found) print "" }' "$metrics_file"
}

endurance_run() {
  local role="$1"
  local config_path="$2"
  local metrics_port="$3"
  local log_file="$LOG_DIR/preview-$role-endurance-$(date +%Y%m%d-%H%M%S).log"
  local metrics_file="$LOG_DIR/preview-$role-endurance-metrics.txt"
  local min_slot_advance="${MIN_SLOT_ADVANCE:-1000}"

  require_prereqs
  require_file "$YGG_BIN" "yggdrasil-node binary"
  require_file "$config_path" "generated $role config"

  echo "[info] starting preview $role endurance for ${RUN_SECONDS}s"
  echo "[info] log: $log_file"

  local extra_args=()
  if [[ "$role" == "relay" ]]; then
    extra_args=(--non-producing-node)
  fi

  set +e
  "$YGG_BIN" run --config "$config_path" --metrics-port "$metrics_port" "${extra_args[@]}" >"$log_file" 2>&1 &
  local pid=$!
  set -e

  cleanup() {
    kill "$pid" >/dev/null 2>&1 || true
    wait "$pid" >/dev/null 2>&1 || true
  }
  trap cleanup EXIT INT TERM

  local elapsed connected metrics_seen producer_started start_slot end_slot
  elapsed=0
  connected=0
  metrics_seen=0
  producer_started=0
  start_slot=""
  end_slot=""

  while [[ "$elapsed" -lt "$RUN_SECONDS" ]]; do
    if ! kill -0 "$pid" >/dev/null 2>&1; then
      echo "ERROR: yggdrasil-node exited early" >&2
      tail -n 100 "$log_file" >&2 || true
      exit 1
    fi
    if rg -q "bootstrap peer connected|verified sync session established" "$log_file" 2>/dev/null; then
      connected=1
    fi
    if [[ "$role" != "producer" || "$producer_started" -eq 0 ]]; then
      if rg -q "Startup.BlockProducer|block producer loop started" "$log_file" 2>/dev/null; then
        producer_started=1
      fi
    fi
    if curl -fsS "http://127.0.0.1:$metrics_port/metrics" >"$metrics_file" 2>/dev/null; then
      if rg -q '^yggdrasil_' "$metrics_file"; then
        metrics_seen=1
        end_slot="$(metric_value "$metrics_file" "yggdrasil_current_slot")"
        if [[ -z "$start_slot" && -n "$end_slot" ]]; then
          start_slot="$end_slot"
        fi
      fi
    fi
    sleep 1
    elapsed=$((elapsed + 1))
  done

  kill "$pid" >/dev/null 2>&1 || true
  wait "$pid" >/dev/null 2>&1 || true
  trap - EXIT INT TERM

  if grep -q "invalid VRF proof" "$log_file"; then
    echo "ERROR: observed invalid VRF proof" >&2
    exit 1
  fi
  if [[ "$connected" -ne 1 || "$metrics_seen" -ne 1 ]]; then
    echo "ERROR: preview $role endurance did not reach connection/metrics gates" >&2
    echo "[info] connected=$connected metrics_seen=$metrics_seen producer_started=$producer_started" >&2
    tail -n 120 "$log_file" >&2 || true
    exit 1
  fi
  if [[ "$role" == "producer" && "$producer_started" -ne 1 ]]; then
    echo "ERROR: producer endurance did not observe block producer startup" >&2
    tail -n 120 "$log_file" >&2 || true
    exit 1
  fi
  if [[ -z "$start_slot" || -z "$end_slot" ]]; then
    echo "ERROR: preview $role endurance could not read slot metrics" >&2
    exit 1
  fi

  local slot_delta=$((end_slot - start_slot))
  if [[ "$slot_delta" -lt "$min_slot_advance" ]]; then
    echo "ERROR: preview $role slot advance $slot_delta < required $min_slot_advance" >&2
    tail -n 120 "$log_file" >&2 || true
    exit 1
  fi

  echo "[ok] preview $role endurance passed"
  echo "[info] slotStart=$start_slot slotEnd=$end_slot slotDelta=$slot_delta"
  echo "[info] log: $log_file"
  echo "[info] metrics:"
  rg '^(yggdrasil_current_slot|yggdrasil_blocks_synced|yggdrasil_batches_completed|yggdrasil_active_peers|yggdrasil_known_peers|yggdrasil_blocks_forged|yggdrasil_blocks_adopted|yggdrasil_reconnects|yggdrasil_rollbacks)' "$metrics_file" || true
  echo "[info] log evidence:"
  rg -i "Startup.BlockProducer|block producer loop started|bootstrap peer connected|sync complete|ledger state is not recent|forged|leader|adopted|not leader|invalid VRF|error" "$log_file" | tail -n 100 || true
}

main() {
  local command="${1:-}"
  case "$command" in
    -h|--help|help)
      usage
      ;;
    generate)
      generate_bundle
      ;;
    wallet)
      generate_wallet
      ;;
    certs)
      generate_registration_certs
      ;;
    funding-address)
      ensure_wallet
      cat "$WALLET_DIR/payment.addr"
      echo
      ;;
    validate)
      validate_bundle
      ;;
    smoke-relay)
      smoke_run "relay" "$CONFIG_DIR/preview-relay.json" "$RELAY_METRICS_PORT"
      ;;
    smoke-producer)
      smoke_run "producer" "$CONFIG_DIR/preview-producer.json" "$PRODUCER_METRICS_PORT"
      ;;
    endurance-relay)
      endurance_run "relay" "$CONFIG_DIR/preview-relay.json" "$RELAY_METRICS_PORT"
      ;;
    endurance-producer)
      endurance_run "producer" "$CONFIG_DIR/preview-producer.json" "$PRODUCER_METRICS_PORT"
      ;;
    all)
      generate_bundle
      generate_wallet
      generate_registration_certs
      validate_bundle
      smoke_run "relay" "$CONFIG_DIR/preview-relay.json" "$RELAY_METRICS_PORT"
      smoke_run "producer" "$CONFIG_DIR/preview-producer.json" "$PRODUCER_METRICS_PORT"
      ;;
    "")
      usage
      exit 1
      ;;
    *)
      echo "ERROR: unknown command: $command" >&2
      usage >&2
      exit 1
      ;;
  esac
}

main "$@"
