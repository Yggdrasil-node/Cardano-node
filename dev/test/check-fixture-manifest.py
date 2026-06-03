#!/usr/bin/env python3
"""Validate the vendored upstream test-vector corpus.

Cross-checks that every authoritative reference to the
`cardano-base` upstream commit SHA agrees:

  1. `crates/node/config/src/upstream_pins.rs::UPSTREAM_CARDANO_BASE_COMMIT`
  2. `dev/specs/upstream-test-vectors/cardano-base/<SHA>/` directory name
  3. `docs/SPECS.md` Vendored Upstream Test Vectors section
  4. `docs/UPSTREAM_PARITY.md` pin matrix

Then verifies that every required fixture corpus directory exists
under the SHA-pinned tree and has its `AGENTS.md` provenance file.

Usage:
    python3 dev/test/check-fixture-manifest.py

Exit codes:
  0 -- manifest is consistent and every expected corpus is present.
  1 -- consistency or corpus-presence error.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
UPSTREAM_PINS_RS = ROOT / "crates" / "node" / "config" / "src" / "upstream_pins.rs"
FIXTURE_TREE = ROOT / "dev" / "specs" / "upstream-test-vectors" / "cardano-base"
SPECS_MD = ROOT / "docs" / "SPECS.md"
UPSTREAM_PARITY_MD = ROOT / "docs" / "UPSTREAM_PARITY.md"

# Corpora that MUST be present under the pinned-SHA tree, with the
# AGENTS.md provenance file for each.
REQUIRED_CORPORA = [
    Path("cardano-crypto-praos") / "test_vectors",
    Path("cardano-crypto-class") / "bls12-381-test-vectors" / "test_vectors",
]


def fail(message: str) -> None:
    print(f"check-fixture-manifest error: {message}", file=sys.stderr)
    raise SystemExit(1)


def info(message: str) -> None:
    print(f"  {message}", file=sys.stderr)


def extract_pins_rs_sha() -> str:
    """Read `UPSTREAM_CARDANO_BASE_COMMIT` from upstream_pins.rs."""
    if not UPSTREAM_PINS_RS.is_file():
        fail(f"missing source-of-truth file: {UPSTREAM_PINS_RS.relative_to(ROOT)}")
    text = UPSTREAM_PINS_RS.read_text(encoding="utf-8")
    pattern = re.compile(
        r'UPSTREAM_CARDANO_BASE_COMMIT\s*:\s*&str\s*=\s*"([0-9a-f]{40})"'
    )
    match = pattern.search(text)
    if not match:
        fail(
            f"could not parse UPSTREAM_CARDANO_BASE_COMMIT in "
            f"{UPSTREAM_PINS_RS.relative_to(ROOT)}"
        )
    return match.group(1)


def discover_fixture_sha_dirs() -> list[str]:
    """Return the list of SHA-shaped directory names under the
    fixture tree.
    """
    if not FIXTURE_TREE.is_dir():
        fail(f"missing fixture tree: {FIXTURE_TREE.relative_to(ROOT)}")
    sha_pattern = re.compile(r"^[0-9a-f]{40}$")
    return sorted(
        [d.name for d in FIXTURE_TREE.iterdir() if d.is_dir() and sha_pattern.match(d.name)]
    )


def check_text_reference(file_path: Path, sha: str, label: str) -> None:
    """Assert the given file mentions the SHA at least once."""
    if not file_path.is_file():
        fail(f"missing {label}: {file_path.relative_to(ROOT)}")
    text = file_path.read_text(encoding="utf-8")
    if sha not in text:
        fail(
            f"{label} ({file_path.relative_to(ROOT)}) does not reference "
            f"the upstream-pins SHA {sha}; the docs and the pin source must "
            f"agree on the same commit hash."
        )


def main() -> None:
    print("Checking fixture manifest consistency:", file=sys.stderr)
    info(f"source of truth: {UPSTREAM_PINS_RS.relative_to(ROOT)}")
    pins_sha = extract_pins_rs_sha()
    info(f"  UPSTREAM_CARDANO_BASE_COMMIT = {pins_sha}")

    fixture_dirs = discover_fixture_sha_dirs()
    if not fixture_dirs:
        fail(
            f"no SHA-shaped directories found under "
            f"{FIXTURE_TREE.relative_to(ROOT)}/. The fixture tree should "
            f"contain at least one directory named after a 40-hex commit "
            f"SHA."
        )
    if len(fixture_dirs) > 1:
        fail(
            f"multiple SHA-shaped directories under "
            f"{FIXTURE_TREE.relative_to(ROOT)}/: {fixture_dirs}. The pin "
            f"policy is single-SHA; remove or merge stale directories "
            f"during a pin refresh."
        )

    fixture_sha = fixture_dirs[0]
    info(f"fixture tree pin: {fixture_sha}")

    if pins_sha != fixture_sha:
        fail(
            f"SHA mismatch: upstream_pins.rs says {pins_sha} but the fixture "
            f"directory is {fixture_sha}. Either rebase the fixture tree to "
            f"the new SHA or update UPSTREAM_CARDANO_BASE_COMMIT to match "
            f"the existing tree."
        )

    info(f"docs reference: {SPECS_MD.relative_to(ROOT)}")
    check_text_reference(SPECS_MD, pins_sha, "docs/SPECS.md")

    info(f"docs reference: {UPSTREAM_PARITY_MD.relative_to(ROOT)}")
    check_text_reference(UPSTREAM_PARITY_MD, pins_sha, "docs/UPSTREAM_PARITY.md")

    sha_dir = FIXTURE_TREE / fixture_sha
    if not (sha_dir / "AGENTS.md").is_file():
        fail(
            f"missing provenance index: {(sha_dir / 'AGENTS.md').relative_to(ROOT)}. "
            f"Each pinned-SHA directory MUST carry an AGENTS.md provenance "
            f"file."
        )

    print(f"\n  required corpora under {sha_dir.relative_to(ROOT)}/:", file=sys.stderr)
    for rel in REQUIRED_CORPORA:
        corpus_path = sha_dir / rel
        if not corpus_path.is_dir():
            fail(
                f"missing required fixture corpus: {corpus_path.relative_to(ROOT)}"
            )
        # Each corpus directory must carry its OWN AGENTS.md
        # provenance (these are the per-corpus operational files).
        sibling_agents = corpus_path.parent / "AGENTS.md"
        if not sibling_agents.is_file():
            fail(
                f"missing per-corpus provenance index: "
                f"{sibling_agents.relative_to(ROOT)}"
            )
        # Empty corpora are suspicious — flag them.
        entries = sorted(p for p in corpus_path.iterdir() if p.is_file())
        if not entries:
            fail(
                f"corpus directory is empty: {corpus_path.relative_to(ROOT)}. "
                f"A vendored corpus must carry at least one fixture file."
            )
        info(f"  {corpus_path.relative_to(ROOT)}: {len(entries)} fixture files")

    print(
        f"\nfixture manifest clean: SHA {pins_sha} consistent across pin "
        f"source, fixture tree, and docs; "
        f"{len(REQUIRED_CORPORA)} corpora validated.",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
