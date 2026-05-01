#!/usr/bin/env bash
# install_from_release.sh — download and install a Yggdrasil release binary.
#
# Usage:
#   ./install_from_release.sh                # latest release
#   ./install_from_release.sh v0.2.0         # specific version
#   ./install_from_release.sh latest /opt    # custom prefix
#
# Detects the host architecture, downloads the matching tarball, verifies its
# SHA256 against the published SHA256SUMS.txt, and installs the binary to
# /usr/local/bin (or the prefix given as the second argument).
#
# Requires: curl, tar, sha256sum (or shasum on macOS).

set -euo pipefail

REPO="${YGGDRASIL_REPO:-yggdrasil-node/Cardano-node}"
VERSION="${1:-latest}"
PREFIX="${2:-/usr/local}"
BINARY_NAME="yggdrasil-node"

err()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }
info() { printf '\033[1;34minfo:\033[0m %s\n' "$*"; }
ok()   { printf '\033[1;32mok:\033[0m %s\n' "$*"; }

require() { command -v "$1" >/dev/null 2>&1 || err "missing required command: $1"; }
require curl
require tar
if command -v sha256sum >/dev/null 2>&1; then
  SHA256_CMD="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
  SHA256_CMD="shasum -a 256"
else
  err "neither sha256sum nor shasum found"
fi

# Detect platform suffix.
case "$(uname -s)" in
  Linux)  os="linux" ;;
  Darwin) err "macOS prebuilt binaries are not yet published; build from source per docs/manual/installation.md" ;;
  *)      err "unsupported OS: $(uname -s)" ;;
esac
case "$(uname -m)" in
  x86_64|amd64)   arch="x86_64" ;;
  aarch64|arm64)  arch="aarch64" ;;
  *)              err "unsupported architecture: $(uname -m)" ;;
esac
suffix="${os}-${arch}"

# Resolve the version tag.
if [ "$VERSION" = "latest" ]; then
  info "resolving latest release..."
  VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
    | grep -oP '"tag_name":\s*"\K[^"]+' | head -1)
  if [ -z "$VERSION" ]; then
    cat >&2 <<EOF
no published GitHub releases found for ${REPO}.

This usually means the GitHub Releases API is temporarily unavailable, or the
repository has not published a release for your architecture. Install from
source:

  git clone https://github.com/${REPO}.git yggdrasil
  cd yggdrasil
  cargo build --release --bin yggdrasil-node
  sudo install -m 0755 target/release/yggdrasil-node /usr/local/bin/

Full instructions: https://yggdrasil-node.github.io/Cardano-node/manual/installation/

If a release does exist and this is a transient API failure, retry, or pass
the tag explicitly: ./install_from_release.sh v0.2.0
EOF
    exit 1
  fi
fi
info "installing ${BINARY_NAME} ${VERSION} (${suffix}) into ${PREFIX}"

archive="${BINARY_NAME}-${VERSION}-${suffix}.tar.gz"
checksum_file="SHA256SUMS.txt"
base_url="https://github.com/${REPO}/releases/download/${VERSION}"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

info "downloading ${archive}..."
curl -fsSL -o "${tmp}/${archive}" "${base_url}/${archive}" \
  || err "could not download ${archive} from ${base_url}"

info "downloading checksums..."
curl -fsSL -o "${tmp}/${checksum_file}" "${base_url}/${checksum_file}" \
  || err "could not download ${checksum_file}"

info "verifying SHA256..."
( cd "$tmp" && grep " ${archive}\$" "${checksum_file}" | ${SHA256_CMD} -c - ) \
  || err "checksum verification failed"
ok "checksum OK"

info "extracting..."
tar -xzf "${tmp}/${archive}" -C "${tmp}"
extracted_dir="${tmp}/${BINARY_NAME}-${VERSION}-${suffix}"
[ -x "${extracted_dir}/${BINARY_NAME}" ] || err "extracted archive missing binary at ${extracted_dir}/${BINARY_NAME}"

bin_target="${PREFIX}/bin/${BINARY_NAME}"
info "installing binary to ${bin_target} (may require sudo)..."
if [ -w "${PREFIX}/bin" ]; then
  install -m 0755 "${extracted_dir}/${BINARY_NAME}" "${bin_target}"
else
  sudo install -m 0755 "${extracted_dir}/${BINARY_NAME}" "${bin_target}"
fi

ok "installed: $("${bin_target}" --version 2>/dev/null || echo "${bin_target}")"
echo
echo "Next steps:"
echo "  ${BINARY_NAME} validate-config --network mainnet --database-path /var/lib/yggdrasil/db"
echo "  ${BINARY_NAME} run             --network mainnet --database-path /var/lib/yggdrasil/db --metrics-port 12798"
echo
echo "User manual: https://yggdrasil-node.github.io/Cardano-node/manual/"
