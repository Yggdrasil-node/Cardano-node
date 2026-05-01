#!/usr/bin/env bash
set -euo pipefail

ACTIONLINT_VERSION="${ACTIONLINT_VERSION:-1.7.8}"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"

if [[ "$(id -u)" -eq 0 ]]; then
  SUDO=()
else
  SUDO=(sudo)
fi

install_shellcheck() {
  if command -v shellcheck >/dev/null 2>&1; then
    echo "[devcontainer] shellcheck already installed: $(shellcheck --version | awk '/^version:/ {print $2}')"
    return
  fi

  echo "[devcontainer] installing shellcheck"
  "${SUDO[@]}" apt-get update
  "${SUDO[@]}" env DEBIAN_FRONTEND=noninteractive \
    apt-get install -y --no-install-recommends shellcheck
  "${SUDO[@]}" rm -rf /var/lib/apt/lists/*
}

install_actionlint() {
  if command -v actionlint >/dev/null 2>&1; then
    installed_version="$(actionlint -version 2>/dev/null | awk 'NR == 1 {print $1}')"
    if [[ "$installed_version" == "$ACTIONLINT_VERSION" ]]; then
      echo "[devcontainer] actionlint $installed_version already installed"
      return
    fi
    echo "[devcontainer] actionlint $installed_version != $ACTIONLINT_VERSION; reinstalling"
  fi

  case "$(uname -m)" in
    x86_64) actionlint_arch="amd64" ;;
    aarch64|arm64) actionlint_arch="arm64" ;;
    *)
      echo "[devcontainer] unsupported actionlint architecture: $(uname -m)" >&2
      exit 1
      ;;
  esac

  tmpdir="$(mktemp -d -t actionlint-install-XXXXXX)"
  (
    trap 'rm -rf "$tmpdir"' EXIT

    asset="actionlint_${ACTIONLINT_VERSION}_linux_${actionlint_arch}.tar.gz"
    url="https://github.com/rhysd/actionlint/releases/download/v${ACTIONLINT_VERSION}/${asset}"

    echo "[devcontainer] installing actionlint $ACTIONLINT_VERSION"
    curl -fsSL "$url" -o "$tmpdir/$asset"
    tar -xzf "$tmpdir/$asset" -C "$tmpdir"
    "${SUDO[@]}" install -m 0755 "$tmpdir/actionlint" /usr/local/bin/actionlint
  )
}

install_static_link_tools() {
  missing=()
  for tool in musl-gcc file; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      missing+=("$tool")
    fi
  done

  if [[ "${#missing[@]}" -gt 0 ]]; then
    echo "[devcontainer] installing static-link helper tools: ${missing[*]}"
    "${SUDO[@]}" apt-get update
    "${SUDO[@]}" env DEBIAN_FRONTEND=noninteractive \
      apt-get install -y --no-install-recommends musl-tools file
    "${SUDO[@]}" rm -rf /var/lib/apt/lists/*
  else
    echo "[devcontainer] static-link helper tools already installed"
  fi

  if rustup target list --installed | grep -qx 'x86_64-unknown-linux-musl'; then
    echo "[devcontainer] rust target x86_64-unknown-linux-musl already installed"
  else
    echo "[devcontainer] installing rust target x86_64-unknown-linux-musl"
    rustup target add x86_64-unknown-linux-musl
  fi
}

install_haskell_cardano_node() {
  if ! "$REPO_ROOT/node/scripts/install_haskell_cardano_node.sh"; then
    echo "[devcontainer] install_haskell_cardano_node failed; re-run manually when network access is available" >&2
  fi
}

install_shellcheck
install_actionlint
install_static_link_tools
install_haskell_cardano_node
