#!/usr/bin/env bash
set -euo pipefail

# Check whether a generated preview pool registration is active yet. This is a
# readiness gate for the producer soak; it does not start a node or claim block
# production evidence.

KOIOS_BASE_URL="${KOIOS_BASE_URL:-https://preview.koios.rest/api/v1}"
POOL_ID="${POOL_ID:-}"
CRED_DIR="${CRED_DIR:-}"
REQUIRE_ACTIVE="${REQUIRE_ACTIVE:-0}"
NETWORK_MAGIC="${NETWORK_MAGIC:-2}"

if [[ -z "$POOL_ID" && -n "$CRED_DIR" && -f "$CRED_DIR/pool.id" ]]; then
  POOL_ID="$(tr -d '\n\r' <"$CRED_DIR/pool.id")"
fi
if [[ -z "$POOL_ID" && -n "$CRED_DIR" && -f "$CRED_DIR/pool.id.bech32" ]]; then
  POOL_ID="$(tr -d '\n\r' <"$CRED_DIR/pool.id.bech32")"
fi

usage() {
  cat <<'EOF'
Usage:
  POOL_ID=pool1... scripts/preview_pool_activation_status.sh

Or with the generated parity-audit credential bundle:
  CRED_DIR=/tmp/ygg-preview-generated-bp-... \
  POOL_ID=pool1... \
  scripts/preview_pool_activation_status.sh

Checks Koios preview `pool_updates` and `tip`, then prints:
  - pool registration tx and active_epoch_no
  - current preview tip epoch/slot
  - seconds/UTC ETA until active, when pending
  - producer credential env exports and sign-off producer_command, when CRED_DIR is supplied

Optional env:
  KOIOS_BASE_URL   Default: https://preview.koios.rest/api/v1
  REQUIRE_ACTIVE   Default: 0. Set 1 to exit non-zero until current epoch >= active epoch.
  NETWORK_MAGIC    Default: 2; this helper is preview-only and rejects other values.

Exit codes:
  0   Pool status read successfully; also active when REQUIRE_ACTIVE=1.
  1   Query failed or pool registration not found.
  2   Bad invocation.
  3   REQUIRE_ACTIVE=1 and the pool is registered but not active yet.
EOF
}

require_bool01() {
  local name="$1"
  local value="$2"
  if [[ "$value" != "0" && "$value" != "1" ]]; then
    echo "ERROR: $name must be 0 or 1, got '$value'" >&2
    return 2
  fi
}

main() {
  if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
  fi
  if [[ "$NETWORK_MAGIC" != "2" ]]; then
    echo "ERROR: preview_pool_activation_status.sh is preview-only; NETWORK_MAGIC must be 2" >&2
    exit 2
  fi
  require_bool01 "REQUIRE_ACTIVE" "$REQUIRE_ACTIVE"
  if [[ -z "$POOL_ID" ]]; then
    echo "ERROR: POOL_ID is required" >&2
    usage >&2
    exit 2
  fi
  if ! command -v curl >/dev/null 2>&1; then
    echo "ERROR: curl is required" >&2
    exit 1
  fi
  if ! command -v python3 >/dev/null 2>&1; then
    echo "ERROR: python3 is required" >&2
    exit 1
  fi

  local tmpdir pool_json tip_json
  tmpdir="$(mktemp -d /tmp/ygg-preview-pool-status.XXXXXX)"
  pool_json="$tmpdir/pool_updates.json"
  tip_json="$tmpdir/tip.json"

  curl -fsS --max-time 30 \
    -H 'accept: application/json' \
    -H 'content-type: application/json' \
    -X POST "$KOIOS_BASE_URL/pool_updates" \
    -d "{\"_pool_bech32\":\"$POOL_ID\"}" \
    >"$pool_json"
  curl -fsS --max-time 20 \
    -H 'accept: application/json' \
    "$KOIOS_BASE_URL/tip" \
    >"$tip_json"

  python3 - "$pool_json" "$tip_json" "$POOL_ID" "$CRED_DIR" "$REQUIRE_ACTIVE" <<'PY'
import datetime
import json
import pathlib
import sys

pool_path, tip_path, pool_id, cred_dir, require_active = sys.argv[1:6]
with open(pool_path, "r", encoding="utf-8") as handle:
    updates = json.load(handle)
with open(tip_path, "r", encoding="utf-8") as handle:
    tip_rows = json.load(handle)

if not updates:
    print(f"ERROR: no pool registration/update found for {pool_id}", file=sys.stderr)
    sys.exit(1)
if not tip_rows:
    print("ERROR: preview tip query returned no rows", file=sys.stderr)
    sys.exit(1)

update = max(
    updates,
    key=lambda row: (
        int(row.get("block_time") or 0),
        int(row.get("active_epoch_no") or 0),
    ),
)
tip = tip_rows[0]
active_epoch = int(update["active_epoch_no"])
current_epoch = int(tip["epoch_no"])
epoch_slot = int(tip["epoch_slot"])
epoch_length = 86400
pending = current_epoch < active_epoch
remaining = max(0, (active_epoch - current_epoch) * epoch_length - epoch_slot)
eta = datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(seconds=remaining)

print(f"pool_id={pool_id}")
print(f"registration_tx={update.get('tx_hash')}")
print(f"update_type={update.get('update_type')}")
print(f"active_epoch_no={active_epoch}")
print(f"current_epoch_no={current_epoch}")
print(f"current_epoch_slot={epoch_slot}")
print(f"current_block_height={tip.get('block_height')}")
print(f"seconds_until_active={remaining}")
print(f"active_eta_utc={eta.replace(microsecond=0).isoformat()}")
print(f"status={'pending' if pending else 'active'}")

if cred_dir:
    root = pathlib.Path(cred_dir)
    print("producer_env:")
    print(f"  KES_SKEY_PATH={root / 'kes.skey'}")
    print(f"  VRF_SKEY_PATH={root / 'vrf.skey'}")
    print(f"  OPCERT_PATH={root / 'node.cert'}")
    print("producer_command:")
    print("  HASKELL_SOCK=/tmp/ygg-haskell-preview/preview/socket/node.socket \\")
    print("  RUN_SECONDS=21600 TIP_COMPARE_CHECKPOINTS=900,3600,21600 \\")
    print("  EXPECT_FORGE_EVENTS=1 EXPECT_ADOPTED_EVENTS=1 REQUIRE_TIP_COMPARISON=1 \\")
    print("  scripts/run_preview_real_pool_producer.sh")

if require_active == "1" and pending:
    sys.exit(3)
PY
}

main "$@"
