#!/usr/bin/env python3
"""Validate the vendored Haskell `cardano-node` install tree.

Checks that `.reference-haskell-cardano-node/install/` has the
binaries, network share dirs, and per-network operator-config files
that Yggdrasil's parity-research and rehearsal scripts expect.
This is a Linux/WSL operator gate because the vendored IntersectMBO
release bundle contains Linux executables.

Concretely:

  1. The `install/bin/cardano-node --version` reports the policy
     reference tag tracked in `docs/parity-matrix.json::reference.tag`
     (currently `11.0.1`).
  2. Every required binary (cardano-node, cardano-cli, db-analyser,
     db-synthesizer, db-truncater, cardano-tracer, ...) is present
     and executable.
  3. The per-network share directories
     `install/share/{mainnet,preprod,preview}/` exist with each
     required operator-config file (config.json, topology.json,
     {byron,shelley,alonzo,conway}-genesis.json,
     peer-snapshot.json, tracer-config.json).

Usage:
    python3 scripts/check-reference-artifacts.py

Exit codes:
  0 -- artifact tree is complete and the version matches the policy tag.
  1 -- missing artifact, version mismatch, or read failure.
"""

from __future__ import annotations

import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PARITY_MATRIX = ROOT / "docs" / "parity-matrix.json"
INSTALL_ROOT = ROOT / ".reference-haskell-cardano-node" / "install"
BIN_DIR = INSTALL_ROOT / "bin"
SHARE_DIR = INSTALL_ROOT / "share"

REQUIRED_BINARIES = [
    "bech32",
    "cardano-cli",
    "cardano-node",
    "cardano-submit-api",
    "cardano-testnet",
    "cardano-tracer",
    "db-analyser",
    "db-synthesizer",
    "db-truncater",
]

REQUIRED_NETWORKS = ["mainnet", "preprod", "preview"]

# Required per-network operator-config files. The list mirrors the
# canonical bundle that Yggdrasil's `configuration/<network>/`
# tree shadows.
REQUIRED_NETWORK_FILES = [
    "config.json",
    "topology.json",
    "byron-genesis.json",
    "shelley-genesis.json",
    "alonzo-genesis.json",
    "conway-genesis.json",
    "peer-snapshot.json",
    "tracer-config.json",
]


def fail(message: str) -> None:
    print(f"check-reference-artifacts error: {message}", file=sys.stderr)
    raise SystemExit(1)


def info(message: str) -> None:
    print(f"  {message}", file=sys.stderr)


def policy_tag() -> str:
    """Read the policy IntersectMBO/cardano-node release tag from
    docs/parity-matrix.json::reference.tag.
    """
    if not PARITY_MATRIX.is_file():
        fail(f"missing {PARITY_MATRIX.relative_to(ROOT)}")
    try:
        data = json.loads(PARITY_MATRIX.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        fail(f"invalid JSON in {PARITY_MATRIX.relative_to(ROOT)}: {exc}")
    reference = data.get("reference")
    if not isinstance(reference, dict):
        fail(f"{PARITY_MATRIX.relative_to(ROOT)} missing top-level `reference` object")
    tag = reference.get("tag")
    if not isinstance(tag, str) or not tag:
        fail(f"{PARITY_MATRIX.relative_to(ROOT)} `reference.tag` must be a non-empty string")
    return tag


def installed_node_version() -> str:
    """Run `cardano-node --version` and parse the version string."""
    binary = BIN_DIR / "cardano-node"
    if not binary.is_file():
        fail(
            f"missing {binary.relative_to(ROOT)}; run "
            f"`bash scripts/setup-reference.sh` to populate the install tree."
        )
    try:
        proc = subprocess.run(
            [str(binary), "--version"],
            check=True,
            capture_output=True,
            text=True,
            timeout=30,
        )
    except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
        fail(f"failed to execute {binary.relative_to(ROOT)} --version: {exc}")
    # Output looks like: `cardano-node 11.0.1 - linux-x86_64 - ghc-9.6\n...`
    match = re.search(r"cardano-node\s+(\S+)", proc.stdout)
    if not match:
        fail(
            f"could not parse version line from "
            f"`{binary.relative_to(ROOT)} --version` output:\n{proc.stdout!r}"
        )
    return match.group(1)


def main() -> None:
    print("Checking reference-artifact tree at "
          f"{INSTALL_ROOT.relative_to(ROOT)}/", file=sys.stderr)

    if not INSTALL_ROOT.is_dir():
        fail(
            f"missing {INSTALL_ROOT.relative_to(ROOT)}; run "
            f"`bash scripts/setup-reference.sh` to populate it."
        )
    if sys.platform != "linux":
        fail(
            "this gate must run under Linux/WSL because "
            ".reference-haskell-cardano-node/install/bin contains Linux "
            "executables. Use `bash scripts/setup-reference.sh --sources-only` "
            "for source/path checks on non-Linux hosts."
        )

    tag = policy_tag()
    info(f"policy reference tag (parity-matrix.json): {tag}")

    installed = installed_node_version()
    info(f"installed cardano-node --version:         {installed}")
    if installed != tag:
        fail(
            f"vendored cardano-node version {installed} does not match the "
            f"policy reference tag {tag}. Run "
            f"`bash scripts/setup-reference.sh --force` to rebase to {tag}."
        )

    print("\n  required binaries:", file=sys.stderr)
    for name in REQUIRED_BINARIES:
        path = BIN_DIR / name
        if not path.is_file():
            fail(f"missing required binary: {path.relative_to(ROOT)}")
        # Check executable bit. Use shutil.which against the install
        # directory rather than os.access for portability.
        if not (path.stat().st_mode & 0o111):
            fail(
                f"binary is not executable: {path.relative_to(ROOT)} "
                f"(mode {oct(path.stat().st_mode)}); run "
                f"`chmod +x {path.relative_to(ROOT)}` or re-extract the "
                f"release tarball."
            )
        info(f"  ✓ {path.relative_to(ROOT)}")

    print("\n  required network share dirs:", file=sys.stderr)
    for net in REQUIRED_NETWORKS:
        net_dir = SHARE_DIR / net
        if not net_dir.is_dir():
            fail(f"missing network share directory: {net_dir.relative_to(ROOT)}")
        for fname in REQUIRED_NETWORK_FILES:
            fpath = net_dir / fname
            if not fpath.is_file():
                fail(
                    f"missing operator-config file: {fpath.relative_to(ROOT)}"
                )
        info(f"  ✓ {net_dir.relative_to(ROOT)}/ ({len(REQUIRED_NETWORK_FILES)} files)")

    print(
        f"\nreference artifacts clean: cardano-node {tag} install + "
        f"{len(REQUIRED_BINARIES)} binaries + "
        f"{len(REQUIRED_NETWORKS)} network share dirs validated.",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
