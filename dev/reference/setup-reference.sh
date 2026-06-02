#!/usr/bin/env bash
# Recreate the reference Haskell tree at .reference-haskell-cardano-node/
# (gitignored sibling of the Rust source tree, ~1.3 GB).
#
# Yggdrasil tracks the latest IntersectMBO/cardano-node release as the parity
# target. Bump CARDANO_NODE_VERSION below whenever upstream ships a new tag,
# and update docs/parity-matrix.json + dev/test/check-parity-matrix.py to match.
#
# Re-run with --force to wipe and start fresh.
# Use --sources-only on non-Linux hosts; the full install downloads and
# executes IntersectMBO's linux-amd64 release bundle.
#
# Disk: ~1.3 GB total (~870 MB binaries + ~370 MB source snapshots).
# Time: ~5 minutes on a typical connection.

set -euo pipefail

VERSION="${CARDANO_NODE_VERSION:-11.0.1}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
REF_DIR="$ROOT_DIR/.reference-haskell-cardano-node"
CARDANO_NODE_REMOTE="https://github.com/IntersectMBO/cardano-node.git"

SOURCES_ONLY=0
for arg in "$@"; do
    case "$arg" in
        --force)
            echo "removing existing $REF_DIR"
            rm -rf "$REF_DIR"
            ;;
        --sources-only)
            # Skip the ~870 MB compiled install tarball; only materialise the
            # upstream source trees needed by parity-flow CI gates
            # (check-parity-matrix.py, check-strict-mirror.py). Used in
            # .github/workflows/ci.yml.
            SOURCES_ONLY=1
            ;;
        *)
            echo "usage: $0 [--force] [--sources-only]" >&2
            exit 2
            ;;
    esac
done

mkdir -p "$REF_DIR"

TMP_ROOT="$(mktemp -d "$REF_DIR/.setup-tmp.XXXXXX")"
cleanup_tmp() {
    rm -rf "$TMP_ROOT"
}
trap cleanup_tmp EXIT

copy_checkout_without_git() {
    local url="$1"
    local dest="$2"
    local ref="${3:-}"
    local tmp

    tmp="$(mktemp -d "$TMP_ROOT/checkout.XXXXXX")"
    if [[ -n "$ref" ]]; then
        git clone --depth 1 --branch "$ref" "$url" "$tmp/repo"
    else
        git clone --depth 1 "$url" "$tmp/repo"
    fi

    mkdir -p "$dest"
    tar -C "$tmp/repo" --exclude=.git -cf - . | tar -C "$dest" -xf -
    rm -rf "$tmp"
}

clean_reference_source_root() {
    find "$REF_DIR" -mindepth 1 -maxdepth 1 \
        ! -name deps \
        ! -name install \
        ! -name "$(basename "$TMP_ROOT")" \
        -exec rm -rf {} +
}

assert_reference_has_no_git_metadata() {
    local leaked

    leaked="$(find "$REF_DIR" -name .git -print -quit)"
    if [[ -n "$leaked" ]]; then
        echo "error: reference tree contains git metadata: ${leaked#$ROOT_DIR/}" >&2
        echo "       re-run with --force so setup-reference.sh can rebuild a metadata-free snapshot." >&2
        exit 1
    fi
}

echo "==> materialising IntersectMBO/cardano-node $VERSION source snapshot"
clean_reference_source_root
copy_checkout_without_git "$CARDANO_NODE_REMOTE" "$REF_DIR" "$VERSION"
printf "%s\n" "$VERSION" > "$REF_DIR/REFERENCE_TAG"

echo "==> cloning upstream library sources into deps/"
mkdir -p "$REF_DIR/deps"
cd "$REF_DIR/deps"
# Format: "<dirname>|<git-url>"
# IntersectMBO repos use the canonical org URL; kes-agent lives under the legacy
# input-output-hk org. bech32, kes-agent, and dmq-node are sister-tool sources
# vendored for the R326-R459 sister-tools port arc — they're consumed via
# cardano-haskell-packages (CHaP) by upstream cardano-node, not via git
# submodules, but Yggdrasil needs the source for strict 1:1 file-mirror parity.
# hermod-tracing carries the `trace-dispatcher` package (cabal name;
# module namespace `Cardano.Logging`, e.g. `Cardano.Logging.Types`).
# As of trace-dispatcher 2.12.x it is no longer in-repo under
# cardano-node/trace-dispatcher/ — it was extracted into the standalone
# IntersectMBO/hermod-tracing repo and is pulled from CHaP by
# trace-forward.cabal (`trace-dispatcher ^>= 2.12`). Yggdrasil's
# cardano-tracer trace-forwarder needs `Cardano.Logging.Types`'
# `Serialise TraceObject` instance for byte-accurate codec parity, so the
# source is vendored here under deps/hermod-tracing/trace-dispatcher/.
for entry in \
    "cardano-base|https://github.com/IntersectMBO/cardano-base.git" \
    "cardano-cli|https://github.com/IntersectMBO/cardano-cli.git" \
    "cardano-ledger|https://github.com/IntersectMBO/cardano-ledger.git" \
    "ouroboros-consensus|https://github.com/IntersectMBO/ouroboros-consensus.git" \
    "ouroboros-network|https://github.com/IntersectMBO/ouroboros-network.git" \
    "plutus|https://github.com/IntersectMBO/plutus.git" \
    "bech32|https://github.com/IntersectMBO/bech32.git" \
    "kes-agent|https://github.com/input-output-hk/kes-agent.git" \
    "dmq-node|https://github.com/IntersectMBO/dmq-node.git" \
    "hermod-tracing|https://github.com/IntersectMBO/hermod-tracing.git" \
; do
    repo="${entry%%|*}"
    url="${entry##*|}"
    echo "    materialising deps/$repo source snapshot"
    rm -rf "$REF_DIR/deps/$repo"
    copy_checkout_without_git "$url" "$REF_DIR/deps/$repo"
done
cd "$REF_DIR"

assert_reference_has_no_git_metadata

if [[ "$SOURCES_ONLY" -eq 1 ]]; then
    echo
    echo "=== reference sources fetched ($VERSION); --sources-only skipped install tarball ==="
    exit 0
fi

if [[ "$(uname -s)" != "Linux" ]]; then
    echo "error: full reference install uses IntersectMBO's linux-amd64 release tarball." >&2
    echo "       Re-run under Linux/WSL, or use --sources-only for the portable source snapshot." >&2
    exit 1
fi

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
# Override DB/socket/log root: RUN_ROOT=/tmp/cardano-reference ./run-node.sh preview
set -euo pipefail
NET="${1:-mainnet}"
PORT="${PORT:-3001}"
case "$NET" in mainnet|preprod|preview) ;;
  *) echo "usage: $0 {mainnet|preprod|preview}" >&2; exit 2 ;;
esac
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUN_BASE="${RUN_ROOT:-$ROOT/run}"
RUN="$RUN_BASE/$NET"
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

assert_reference_has_no_git_metadata

echo
echo "=== reference setup complete (cardano-node $VERSION) ==="
echo "Run a relay-only mainnet node:"
echo "    $REF_DIR/install/run-node.sh mainnet"
