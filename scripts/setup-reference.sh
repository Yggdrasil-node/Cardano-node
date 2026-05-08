#!/usr/bin/env bash
# Recreate the reference Haskell tree at .reference-haskell-cardano-node/
# (gitignored sibling of the Rust source tree, ~1.3 GB).
#
# Yggdrasil tracks the latest IntersectMBO/cardano-node release as the parity
# target. Bump CARDANO_NODE_VERSION below whenever upstream ships a new tag,
# and update docs/parity-matrix.json + scripts/check-parity-matrix.py to match.
#
# Re-run with --force to wipe and start fresh.
#
# Disk: ~1.3 GB total (~870 MB binaries + ~370 MB source clones).
# Time: ~5 minutes on a typical connection.

set -euo pipefail

VERSION="${CARDANO_NODE_VERSION:-11.0.1}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REF_DIR="$ROOT_DIR/.reference-haskell-cardano-node"
CARDANO_NODE_REMOTE="https://github.com/IntersectMBO/cardano-node.git"

if [[ "${1:-}" == "--force" ]]; then
    echo "removing existing $REF_DIR"
    rm -rf "$REF_DIR"
fi

mkdir -p "$REF_DIR"
cd "$REF_DIR"

echo "==> materialising IntersectMBO/cardano-node $VERSION source"
if [[ -d .git ]]; then
    if git remote get-url origin >/dev/null 2>&1; then
        git remote set-url origin "$CARDANO_NODE_REMOTE"
    else
        git remote add origin "$CARDANO_NODE_REMOTE"
    fi
    git fetch --tags --depth 1 origin "$VERSION"
    git checkout --detach "$VERSION"
else
    git clone --depth 1 --branch "$VERSION" "$CARDANO_NODE_REMOTE" .
fi

echo "==> cloning upstream library sources into deps/"
mkdir -p deps
cd deps
for repo in cardano-base cardano-cli cardano-ledger ouroboros-consensus ouroboros-network plutus; do
    if [[ -d "$repo/.git" ]]; then
        echo "    deps/$repo already present, refreshing tags"
        git -C "$repo" fetch --tags --depth 1 origin
    else
        git clone --depth 1 "https://github.com/IntersectMBO/$repo.git"
    fi
done
cd ..

echo "==> downloading cardano-node $VERSION release tarball"
mkdir -p install
cd install
TARBALL="cardano-node-$VERSION-linux-amd64.tar.gz"
SUMS="cardano-node-$VERSION-sha256sums.txt"
[[ -f "$TARBALL" ]] || curl -sS -L -O "https://github.com/IntersectMBO/cardano-node/releases/download/$VERSION/$TARBALL"
[[ -f "$SUMS"    ]] || curl -sS -L -O "https://github.com/IntersectMBO/cardano-node/releases/download/$VERSION/$SUMS"

echo "==> verifying SHA-256"
grep "linux-amd64.tar.gz" "$SUMS" | sha256sum -c -

echo "==> extracting"
rm -rf bin share
tar -xzf "$TARBALL"
rm "$TARBALL"
cd ..

echo "==> verifying binaries run"
./install/bin/cardano-node --version

cat > install/run-node.sh <<'LAUNCHER'
#!/usr/bin/env bash
# Launcher for the reference cardano-node.
#   ./run-node.sh mainnet|preprod|preview
# Override port: PORT=3002 ./run-node.sh preprod
set -euo pipefail
NET="${1:-mainnet}"
PORT="${PORT:-3001}"
case "$NET" in mainnet|preprod|preview) ;;
  *) echo "usage: $0 {mainnet|preprod|preview}" >&2; exit 2 ;;
esac
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUN="$ROOT/run/$NET"
mkdir -p "$RUN/db" "$RUN/socket" "$RUN/log"
exec "$ROOT/bin/cardano-node" run \
    --config        "$ROOT/share/$NET/config.json" \
    --topology      "$ROOT/share/$NET/topology.json" \
    --database-path "$RUN/db" \
    --socket-path   "$RUN/socket/node.socket" \
    --port          "$PORT" \
    --start-as-non-producing-node
LAUNCHER
chmod +x install/run-node.sh

echo
echo "=== reference setup complete (cardano-node $VERSION) ==="
echo "Run a relay-only mainnet node:"
echo "    $REF_DIR/install/run-node.sh mainnet"
