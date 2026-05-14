#!/usr/bin/env bash
set -euo pipefail

# Install the upstream IntersectMBO Haskell `cardano-node` + `cardano-cli`
# binaries into `~/.local/bin/` for use by §5 hash-comparison runbook
# (`compare_tip_to_haskell.sh`) and §6.5b parallel-fetch parity check.
#
# Idempotent — skips download if the requested version is already
# installed.  Use `--force` to reinstall.
#
# Usage:
#   node/scripts/install_haskell_cardano_node.sh           # install latest
#   CARDANO_NODE_VERSION=10.7.1 node/scripts/install_haskell_cardano_node.sh
#   node/scripts/install_haskell_cardano_node.sh --force
#
# Exit codes:
#   0  installed (or already present, version matches)
#   1  download / extraction failure
#   2  version mismatch after install (corrupt tarball)
#   3  unsupported architecture

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64)  ASSET_ARCH="linux-amd64" ;;
  aarch64) ASSET_ARCH="linux-arm64" ;;
  *)
    echo "ERROR: unsupported arch $ARCH" >&2
    exit 3
    ;;
esac

INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
FORCE=0
for arg in "$@"; do
  case "$arg" in
    --force) FORCE=1 ;;
    -h|--help)
      sed -n '/^# Usage:/,/^# Exit codes:/p' "$0" | sed -e 's/^# \{0,1\}//'
      exit 0
      ;;
  esac
done

if [[ -z "${CARDANO_NODE_VERSION:-}" ]]; then
  if ! CARDANO_NODE_VERSION="$(curl -sS \
      'https://api.github.com/repos/IntersectMBO/cardano-node/releases/latest' \
      | jq -r '.tag_name')"; then
    echo "ERROR: could not resolve latest IOG cardano-node version" >&2
    exit 1
  fi
fi

echo "[install_haskell_cardano_node] target version: $CARDANO_NODE_VERSION"
echo "[install_haskell_cardano_node] arch:           $ASSET_ARCH"
echo "[install_haskell_cardano_node] install dir:    $INSTALL_DIR"

mkdir -p "$INSTALL_DIR"

# Idempotency: skip if already installed at the right version.
if [[ "$FORCE" -eq 0 ]] && command -v cardano-cli >/dev/null 2>&1; then
  installed_ver="$(cardano-cli --version 2>/dev/null \
    | head -1 | awk '{print $2}' || echo unknown)"
  if [[ "$installed_ver" == "$CARDANO_NODE_VERSION" ]]; then
    echo "[install_haskell_cardano_node] cardano-cli $installed_ver already installed; skipping"
    exit 0
  fi
  echo "[install_haskell_cardano_node] installed version $installed_ver != target $CARDANO_NODE_VERSION; reinstalling"
fi

ASSET="cardano-node-${CARDANO_NODE_VERSION}-${ASSET_ARCH}.tar.gz"
URL="https://github.com/IntersectMBO/cardano-node/releases/download/${CARDANO_NODE_VERSION}/${ASSET}"

TMPDIR="$(mktemp -d -t cardano-node-install-XXXXXX)"
trap "rm -rf '$TMPDIR'" EXIT

echo "[install_haskell_cardano_node] fetching $URL"
if ! curl -fL --progress-bar -o "$TMPDIR/$ASSET" "$URL"; then
  echo "ERROR: download failed" >&2
  exit 1
fi

echo "[install_haskell_cardano_node] extracting"
if ! tar -xzf "$TMPDIR/$ASSET" -C "$TMPDIR"; then
  echo "ERROR: extraction failed" >&2
  exit 1
fi

# IOG tarballs put binaries directly at the root or under bin/.  Find them.
BINARIES=(cardano-cli cardano-node)
for bin in "${BINARIES[@]}"; do
  src="$(find "$TMPDIR" -name "$bin" -type f -executable -print -quit 2>/dev/null || true)"
  if [[ -z "$src" ]]; then
    echo "ERROR: could not find $bin in extracted tarball" >&2
    exit 2
  fi
  install -m 0755 "$src" "$INSTALL_DIR/$bin"
  echo "[install_haskell_cardano_node] installed $INSTALL_DIR/$bin"
done

echo
echo "=== verification ==="
"$INSTALL_DIR/cardano-cli" --version
"$INSTALL_DIR/cardano-node" --version
echo
echo "[install_haskell_cardano_node] OK — cardano-cli + cardano-node $CARDANO_NODE_VERSION installed"
echo "[install_haskell_cardano_node] add to PATH if not already: export PATH=\"$INSTALL_DIR:\$PATH\""
