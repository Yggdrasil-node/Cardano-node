#!/usr/bin/env bash
# backup_db.sh — snapshot a Yggdrasil chain database.
#
# Stops the node, tars the DB to a destination directory, then restarts
# the node. For hot snapshots without downtime, see docs/manual/maintenance.md
# (LVM/ZFS snapshot procedure).
#
# Usage:
#   ./backup_db.sh /var/lib/yggdrasil/db /backup
#   ./backup_db.sh /var/lib/yggdrasil/db /backup --service yggdrasil-node
#
# Defaults:
#   DB_PATH=/var/lib/yggdrasil/db
#   DEST_DIR=/backup
#   SERVICE=yggdrasil-node
#
# Exit codes:
#   0 on success, 1 on any failure (the node is restarted before exit).

set -euo pipefail

DB_PATH="${1:-/var/lib/yggdrasil/db}"
DEST_DIR="${2:-/backup}"
SERVICE="yggdrasil-node"

# Optional override flags.
shift 2 || true
while [ "$#" -gt 0 ]; do
  case "$1" in
    --service) SERVICE="$2"; shift 2 ;;
    *) printf 'unknown option: %s\n' "$1" >&2; exit 2 ;;
  esac
done

err()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; }
info() { printf '\033[1;34minfo:\033[0m %s\n' "$*"; }
ok()   { printf '\033[1;32mok:\033[0m %s\n' "$*"; }

[ -d "$DB_PATH" ] || { err "DB path not found: $DB_PATH"; exit 1; }
[ -d "$DEST_DIR" ] || { err "destination directory not found: $DEST_DIR"; exit 1; }

stamp=$(date +%Y%m%d-%H%M%S)
archive="${DEST_DIR}/yggdrasil-db-${stamp}.tar.gz"

info "stopping ${SERVICE}..."
sudo systemctl stop "$SERVICE"

# Always restart the service even if the snapshot fails.
trap 'info "restarting ${SERVICE}..."; sudo systemctl start "$SERVICE" || err "failed to restart ${SERVICE}"' EXIT

info "creating snapshot: ${archive}"
parent=$(dirname "$DB_PATH")
basename=$(basename "$DB_PATH")
sudo tar -czf "$archive" -C "$parent" "$basename"

info "computing SHA256..."
sudo sha256sum "$archive" | tee "${archive}.sha256"

ok "snapshot complete: $(du -h "$archive" | cut -f1) at ${archive}"
