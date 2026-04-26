#!/usr/bin/env bash
set -euo pipefail

# Compare each pinned upstream IntersectMBO commit (from
# node/src/upstream_pins.rs) against the live HEAD of the corresponding
# repository. Outputs a JSON drift report.
#
# Drift is INFORMATIONAL — this script never sets a non-zero exit on
# drift alone. The audit baseline is allowed to lag upstream; what
# matters is that the lag is visible.
#
# Usage:
#   node/scripts/check_upstream_drift.sh                # human-readable summary
#   node/scripts/check_upstream_drift.sh --json         # JSON only
#   node/scripts/check_upstream_drift.sh --fail-on-drift  # exit 1 if any drift
#
# Exit codes:
#   0  drift report produced successfully (drift may exist; see output)
#   1  --fail-on-drift was set AND at least one repo drifted
#   2  could not fetch live HEAD for one or more repos
#   3  could not parse pinned SHAs from node/src/upstream_pins.rs

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PINS_FILE="$ROOT_DIR/node/src/upstream_pins.rs"

JSON_ONLY=0
FAIL_ON_DRIFT=0

for arg in "$@"; do
  case "$arg" in
    --json) JSON_ONLY=1 ;;
    --fail-on-drift) FAIL_ON_DRIFT=1 ;;
    -h|--help)
      sed -n '/^# Usage:/,/^# Exit codes:/p' "$0" | sed -e 's/^# \{0,1\}//'
      exit 0
      ;;
    *) echo "unknown arg: $arg" >&2; exit 3 ;;
  esac
done

if [[ ! -f "$PINS_FILE" ]]; then
  echo "ERROR: pinned-SHA source file not found at $PINS_FILE" >&2
  exit 3
fi

# Parse `pub const UPSTREAM_<NAME>_COMMIT: &str = "<sha>";` lines into
# (repo, sha) pairs. Repo is derived by lowercasing and replacing _ with -.
parse_pin() {
  local const_name="$1"
  grep -E "^pub const ${const_name}: &str = \"[a-f0-9]{40}\";" "$PINS_FILE" \
    | sed -E "s/^pub const ${const_name}: &str = \"([a-f0-9]{40})\";.*/\1/"
}

declare -A pinned
pinned[cardano-base]="$(parse_pin UPSTREAM_CARDANO_BASE_COMMIT)"
pinned[cardano-ledger]="$(parse_pin UPSTREAM_CARDANO_LEDGER_COMMIT)"
pinned[ouroboros-consensus]="$(parse_pin UPSTREAM_OUROBOROS_CONSENSUS_COMMIT)"
pinned[ouroboros-network]="$(parse_pin UPSTREAM_OUROBOROS_NETWORK_COMMIT)"
pinned[plutus]="$(parse_pin UPSTREAM_PLUTUS_COMMIT)"
pinned[cardano-node]="$(parse_pin UPSTREAM_CARDANO_NODE_COMMIT)"

for repo in "${!pinned[@]}"; do
  if [[ -z "${pinned[$repo]}" ]]; then
    echo "ERROR: could not parse pinned SHA for $repo from $PINS_FILE" >&2
    exit 3
  fi
done

# Fetch live HEAD for each repo. We try main first, then master.
fetch_head() {
  local repo="$1"
  local url="https://github.com/IntersectMBO/${repo}.git"
  for branch in main master; do
    local sha
    sha="$(git ls-remote --heads "$url" 2>/dev/null \
      | awk -v b="refs/heads/${branch}" '$2 == b { print $1; exit }')"
    if [[ -n "$sha" ]]; then
      echo "$sha"
      return 0
    fi
  done
  return 1
}

declare -A live
fetch_failed=0
for repo in cardano-base cardano-ledger ouroboros-consensus ouroboros-network plutus cardano-node; do
  if sha="$(fetch_head "$repo")"; then
    live[$repo]="$sha"
  else
    live[$repo]=""
    fetch_failed=1
  fi
done

# Build JSON report.
ts="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
json="{\"timestamp\":\"$ts\",\"pins\":["
first=1
drifted_count=0
unreachable_count=0
for repo in cardano-base cardano-ledger ouroboros-consensus ouroboros-network plutus cardano-node; do
  pinned_sha="${pinned[$repo]}"
  live_sha="${live[$repo]}"
  drifted="false"
  reachable="true"
  if [[ -z "$live_sha" ]]; then
    reachable="false"
    unreachable_count=$((unreachable_count + 1))
  elif [[ "$pinned_sha" != "$live_sha" ]]; then
    drifted="true"
    drifted_count=$((drifted_count + 1))
  fi

  if [[ "$first" -eq 0 ]]; then
    json+=","
  fi
  first=0
  json+="{\"repo\":\"$repo\",\"pinned_sha\":\"$pinned_sha\",\"live_sha\":\"$live_sha\",\"reachable\":$reachable,\"drifted\":$drifted}"
done
json+="],\"summary\":{\"total\":6,\"drifted\":$drifted_count,\"unreachable\":$unreachable_count}}"

if [[ "$JSON_ONLY" -eq 1 ]]; then
  echo "$json"
else
  echo "[upstream-drift] timestamp=$ts"
  printf "  %-22s  %-7s  %s\n" "repo" "status" "pinned -> live"
  for repo in cardano-base cardano-ledger ouroboros-consensus ouroboros-network plutus cardano-node; do
    pinned_sha="${pinned[$repo]}"
    live_sha="${live[$repo]}"
    if [[ -z "$live_sha" ]]; then
      printf "  %-22s  %-7s  %s -> (unreachable)\n" "$repo" "ERROR" "${pinned_sha:0:12}"
    elif [[ "$pinned_sha" == "$live_sha" ]]; then
      printf "  %-22s  %-7s  %s\n" "$repo" "in-sync" "${pinned_sha:0:12}"
    else
      printf "  %-22s  %-7s  %s -> %s\n" "$repo" "DRIFT" "${pinned_sha:0:12}" "${live_sha:0:12}"
    fi
  done
  echo ""
  echo "[summary] drifted=$drifted_count unreachable=$unreachable_count total=6"
  echo ""
  echo "Drift is informational. To advance a pin: edit"
  echo "node/src/upstream_pins.rs, run the audit cadence against the new"
  echo "SHA, and update docs/UPSTREAM_PARITY.md with the rationale."
fi

if [[ "$FAIL_ON_DRIFT" -eq 1 && "$drifted_count" -gt 0 ]]; then
  exit 1
fi
if [[ "$fetch_failed" -ne 0 ]]; then
  exit 2
fi
exit 0
