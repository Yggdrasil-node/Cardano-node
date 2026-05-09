#!/usr/bin/env python3
"""Strict-mirror discovery audit for Yggdrasil's R274 round.

Walks every production `.rs` under `crates/<crate>/src/` and `node/src/`,
derives candidate upstream basenames via snake_case to PascalCase, looks
each candidate up against the upstream Haskell tree, checks whether the
Rust file already carries a `## Naming parity` docstring stanza, and
emits a TSV at `docs/strict-mirror-audit.tsv` for human grading.

Verdicts (initial, pre-grading):
  candidate_match    - one or more upstream .hs files match a candidate.
  no_candidate_match - no upstream match; needs `## Naming parity` block
                       or a rename in Phase B.

Final per-row verdicts (assigned by hand-grading after this script runs):
  (a) DIRECT_MIRROR              - one upstream .hs matches in name AND
                                   concept; no action needed.
  (b) RENAME_NEEDED              - Yggdrasil-invented name but a real
                                   upstream parent exists; rename in
                                   Phase B.
  (c) NO_MIRROR_NEEDS_DOCSTRING  - genuine synthesis; needs `## Naming
                                   parity` block in Phase B.
  (d) NAME_CLASH_REGRADE         - basename collision with an upstream
                                   file of a different concept; either
                                   rename to disambiguate or annotate.

Usage:
    python3 scripts/audit-strict-mirror.py
    python3 scripts/audit-strict-mirror.py --rebuild-index

Exit codes:
  0 - audit ran cleanly (TSV may contain rows needing grading).
  1 - configuration error (missing reference tree, missing index, etc.).
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REF_DIR = ROOT / ".reference-haskell-cardano-node"
INDEX_PATH = ROOT / "docs" / "upstream-haskell-files.txt"
OUT_PATH = ROOT / "docs" / "strict-mirror-audit.tsv"

RUST_ROOTS = [
    ROOT / "crates",
    ROOT / "node" / "src",
]

def pascal_to_snake_variants(basename: str) -> list[str]:
    """Convert an upstream PascalCase basename to candidate snake_case forms.

    Returns a list of variants because Yggdrasil's snake_case naming is not
    perfectly mechanical:
      - "LedgerDB"   -> ["ledger_db", "ledgerdb"]
      - "ChainDB"    -> ["chain_db", "chaindb"]
      - "OCert"      -> ["o_cert", "ocert"]
      - "BLS12_381"  -> ["bls12_381"]
      - "FromCBOR"   -> ["from_cbor", "fromcbor"]
      - "Ed25519"    -> ["ed25519"]
    The first variant is the strict camelCase-aware form; the second is the
    plain lowercase concatenation that Yggdrasil sometimes uses for short
    acronym-prefixed names ("ocert" instead of "o_cert").
    """
    # Strict: insert _ at camelCase boundaries.
    # Step 1: HTTPSConn -> HTTPS_Conn (consecutive caps followed by a cap+lower).
    s = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1_\2", basename)
    # Step 2: someThing -> some_Thing (lower/digit followed by a cap).
    s = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", s)
    strict = s.lower()
    # Loose: collapse all underscores not surrounded by digits.
    # We keep underscores between digits (BLS12_381 -> bls12_381).
    loose = re.sub(r"(?<![0-9])_(?![0-9])", "", strict)
    out = [strict]
    if loose != strict:
        out.append(loose)
    return out


def derive_candidates(rust_stem: str) -> list[str]:
    """Derive search keys for a Rust file stem.

    The Rust file may match an upstream basename whose snake_case form is
    either a perfect match (`ledger_db` <-> `LedgerDB`) or a loose-form
    match (`ocert` <-> `OCert`). The reverse index built in build_index
    contains both variants, so we just return the rust stem itself plus a
    last-component fallback for nested paths.
    """
    out = [rust_stem]
    if "_" in rust_stem:
        out.append(rust_stem.split("_")[-1])
    return out


def build_index(rebuild: bool) -> dict[str, list[str]]:
    """Build {basename_no_ext: [relative_paths]} from upstream tree."""
    if INDEX_PATH.exists() and not rebuild:
        return load_index()
    if not REF_DIR.exists():
        print(
            f"audit-strict-mirror error: {REF_DIR} not present; "
            "run `bash scripts/setup-reference.sh --force` first.",
            file=sys.stderr,
        )
        raise SystemExit(1)
    print(f"  building index from {REF_DIR}", file=sys.stderr)
    proc = subprocess.run(
        ["find", str(REF_DIR), "-name", "*.hs"],
        check=True,
        capture_output=True,
        text=True,
    )
    paths = [line for line in proc.stdout.splitlines() if line]
    paths.sort()
    INDEX_PATH.parent.mkdir(parents=True, exist_ok=True)
    INDEX_PATH.write_text(
        "\n".join(Path(p).relative_to(ROOT).as_posix() for p in paths) + "\n",
        encoding="utf-8",
    )
    return load_index()


def load_index() -> dict[str, list[str]]:
    """Load the upstream basename index keyed by snake_case variants.

    Each upstream `.hs` file contributes 1-2 keys (strict and loose forms)
    pointing at the upstream relative path.
    """
    if not INDEX_PATH.exists():
        print(
            f"audit-strict-mirror error: {INDEX_PATH} not present; "
            "rerun with --rebuild-index or run setup-reference.sh.",
            file=sys.stderr,
        )
        raise SystemExit(1)
    out: dict[str, list[str]] = defaultdict(list)
    for line in INDEX_PATH.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        stem = Path(line).stem
        # Index by both strict and loose snake_case variants.
        for variant in pascal_to_snake_variants(stem):
            if line not in out[variant]:
                out[variant].append(line)
    return out


PARITY_PATTERN = re.compile(r"##\s+Naming parity", re.IGNORECASE)
STRICT_NONE_PATTERN = re.compile(r"\*\*Strict mirror:\*\*\s+none\.", re.IGNORECASE)
STRICT_MIRROR_DECL = re.compile(r"\*\*Strict mirror:\*\*\s+", re.IGNORECASE)
STRICT_PARTIAL_PATTERN = re.compile(
    r"\*\*Strict mirror\s*\(partial\)\*\*", re.IGNORECASE
)

# Path fragments that indicate an upstream file is NOT a canonical
# production-code mirror (test harnesses, benchmarks, demo apps,
# documentation samples). When filtering candidate hits to find the
# canonical production mirror, we drop matches whose path includes
# any of these fragments.
NON_PRODUCTION_FRAGMENTS = (
    "/test/",
    "/tests/",
    "/test-",
    "/testlib/",
    "/bench/",
    "/benchmarks/",
    "/golden/",
    "/demo/",
    "/notes/",
    "/docs/",
    "/docusaurus/",
    "/sample/",
    "/example/",
    "/app/",
)


def filter_production_hits(hits: list[str]) -> list[str]:
    """Remove non-production paths (test/bench/demo/etc.) from the hit list."""
    keep: list[str] = []
    for h in hits:
        lower = h.lower()
        if any(frag in lower for frag in NON_PRODUCTION_FRAGMENTS):
            continue
        keep.append(h)
    return keep


# Each Yggdrasil crate maps primarily to one upstream repo. When a Rust
# file has multiple upstream-tree hits, the affinity-matching hit is the
# canonical mirror. The list maps `(rust_path_prefix, [upstream_path_substr])`;
# the first matching prefix wins.
CRATE_AFFINITY: list[tuple[str, list[str]]] = [
    ("crates/consensus/src/", ["/ouroboros-consensus/", "/cardano-protocol-tpraos/"]),
    ("crates/network/src/", ["/ouroboros-network/", "/cardano-diffusion/", "/network-mux/"]),
    ("crates/ledger/src/", ["/cardano-ledger/"]),
    ("crates/storage/src/", ["/ouroboros-consensus/.../Storage/", "/ouroboros-consensus/"]),
    ("crates/plutus/src/", ["/plutus/"]),
    ("crates/crypto/src/", ["/cardano-base/cardano-crypto", "/cardano-base/"]),
    ("node/src/", ["/cardano-node/", "/cardano-tracer/"]),
]


def affinity_filter(rust_path: str, hits: list[str]) -> list[str]:
    """Filter hits to those that match the rust_path's crate affinity.

    Returns a non-empty subset if the affinity matches at least one hit;
    otherwise returns the input unchanged.
    """
    rel_str = rust_path
    for prefix, substrs in CRATE_AFFINITY:
        if rel_str.startswith(prefix):
            preferred: list[str] = []
            for h in hits:
                for sub in substrs:
                    # crude substring match on the upstream path
                    if sub.replace("/.../", "/") in h:
                        preferred.append(h)
                        break
            if preferred:
                return preferred
            return hits
    return hits


def auto_grade(
    rust_path: str, hits: list[str], parity_state: str, initial_verdict: str
) -> tuple[str, str]:
    """Produce a provisional final verdict + notes for a row.

    Authoring-side `## Naming parity` blocks are treated as authoritative:
    the file's authors explicitly declared the naming-parity story, so the
    auto-grader does not second-guess. The block's `**Strict mirror:**`
    declaration determines the verdict bucket:
      - `**Strict mirror:** <upstream-path>` -> (a) DIRECT_MIRROR
      - `**Strict mirror (partial)** ...`    -> (c) partial-synthesis
      - `**Strict mirror:** none.`           -> (c) synthesis
      - heading without declaration line     -> (c) unspecified-but-acknowledged
    """
    if parity_state == "yes(strict-mirror)":
        return (
            "(a) DIRECT_MIRROR (auto: docstring declares strict mirror)",
            "verified at audit time; `## Naming parity` block names the upstream `.hs`",
        )
    if parity_state.startswith("yes"):
        kind = parity_state[len("yes"):]  # "(strict-none)", "(strict-partial)", "(unspecified)"
        return (
            f"(c) NO_MIRROR_NEEDS_DOCSTRING (auto: docstring present {kind})",
            "verified at audit time; `## Naming parity` block declares synthesis story",
        )
    production_hits = filter_production_hits(hits)
    if initial_verdict == "no_candidate_match":
        return (
            "(c-needed) NO_MIRROR_NEEDS_DOCSTRING (auto: no upstream + no docstring)",
            "needs `## Naming parity` block in Phase B or rename if a sibling upstream exists",
        )
    if not production_hits:
        return (
            "(NEEDS-REVIEW) all candidate hits are in non-production trees",
            "raw hits all under test/bench/demo/etc.",
        )
    # Apply crate-affinity filter to disambiguate multi-hit matches.
    affinity_hits = affinity_filter(rust_path, production_hits)
    if len(affinity_hits) == 1:
        suffix = " (affinity-filtered)" if len(production_hits) > 1 else ""
        return (
            f"(a) DIRECT_MIRROR (auto{suffix})",
            f"canonical hit: {affinity_hits[0]}",
        )
    if len(production_hits) == 1:
        return (
            "(a) DIRECT_MIRROR (auto)",
            f"unique production hit: {production_hits[0]}",
        )
    return (
        f"(NEEDS-REVIEW) {len(affinity_hits)} affinity-filtered hits "
        f"of {len(production_hits)} production",
        f"first 3: {';'.join(affinity_hits[:3])}",
    )


def has_naming_parity_block(rust_path: Path) -> str:
    """Return one of: 'no', 'yes(strict-none)', 'yes(strict-partial)',
    'yes(strict-mirror)', 'yes(unspecified)'.

    The four 'yes' variants distinguish how the docstring author
    declared the mirror story:
      - strict-none      -> `**Strict mirror:** none.` (synthesis)
      - strict-partial   -> `**Strict mirror (partial)**` (subset/combine)
      - strict-mirror    -> `**Strict mirror:** <upstream-path>` (direct)
      - unspecified      -> heading present but no `**Strict mirror:**` line
    """
    text = rust_path.read_text(encoding="utf-8", errors="replace")
    if not PARITY_PATTERN.search(text):
        return "no"
    if STRICT_NONE_PATTERN.search(text):
        return "yes(strict-none)"
    if STRICT_PARTIAL_PATTERN.search(text):
        return "yes(strict-partial)"
    if STRICT_MIRROR_DECL.search(text):
        return "yes(strict-mirror)"
    return "yes(unspecified)"


def iter_rust_files() -> list[Path]:
    out: list[Path] = []
    for root in RUST_ROOTS:
        if not root.exists():
            continue
        for path in root.rglob("*.rs"):
            # Skip test trees.
            parts = set(path.parts)
            if "tests" in parts or "target" in parts:
                continue
            # Skip crate-roots and wiring shells (not strict-mirror candidates).
            if path.name in {"lib.rs", "main.rs", "mod.rs", "build.rs"}:
                continue
            # Skip unit-test modules at any level. Convention: `tests.rs`
            # (sibling to a module file) and `*_tests.rs` (e.g.
            # `node/src/main_tests.rs`). Tests are inline #[cfg(test)]
            # modules in Yggdrasil and never strict-mirror upstream files.
            if path.name == "tests.rs" or path.name.endswith("_tests.rs"):
                continue
            out.append(path)
    out.sort()
    return out


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Strict-mirror discovery audit (R274)."
    )
    parser.add_argument(
        "--rebuild-index",
        action="store_true",
        help="rebuild docs/upstream-haskell-files.txt from the live tree.",
    )
    args = parser.parse_args()

    index = build_index(args.rebuild_index)
    rust_files = iter_rust_files()

    rows = []
    counts: dict[str, int] = defaultdict(int)
    for rust_path in rust_files:
        rel = rust_path.relative_to(ROOT).as_posix()
        stem = rust_path.stem
        candidates = derive_candidates(stem)
        hits: list[str] = []
        matched_candidate = ""
        seen_paths: set[str] = set()
        for cand in candidates:
            if cand in index:
                if not matched_candidate:
                    matched_candidate = cand
                for p in index[cand]:
                    if p not in seen_paths:
                        seen_paths.add(p)
                        hits.append(p)
        parity_state = has_naming_parity_block(rust_path)
        if hits:
            initial_verdict = "candidate_match"
        else:
            initial_verdict = "no_candidate_match"
        counts[initial_verdict] += 1
        final_verdict, notes = auto_grade(rel, hits, parity_state, initial_verdict)
        # Tally final-verdict bucket for the summary.
        bucket = (
            final_verdict.split(" ")[0]
            if final_verdict
            else "(empty)"
        )
        counts[f"final::{bucket}"] += 1
        # Trim hits to first 3 to keep TSV legible; the TSV is for grading.
        hits_field = ";".join(hits[:3]) + ("..." if len(hits) > 3 else "")
        rows.append(
            "\t".join(
                [
                    rel,
                    "|".join(candidates),
                    matched_candidate or "-",
                    hits_field or "-",
                    parity_state,
                    initial_verdict,
                    final_verdict,
                    notes,
                ]
            )
        )

    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    header = "\t".join(
        [
            "rust_path",
            "candidates",
            "matched_candidate",
            "upstream_hits",
            "docstring_parity",
            "initial_verdict",
            "final_verdict",
            "notes",
        ]
    )
    OUT_PATH.write_text(header + "\n" + "\n".join(rows) + "\n", encoding="utf-8")

    print(
        f"  audit complete: {len(rust_files)} rust files; "
        f"candidate_match={counts['candidate_match']}, "
        f"no_candidate_match={counts['no_candidate_match']}",
        file=sys.stderr,
    )
    print("  auto-grading bucket counts:", file=sys.stderr)
    for key in sorted(counts):
        if key.startswith("final::"):
            print(f"    {key[7:]}: {counts[key]}", file=sys.stderr)
    print(f"  TSV written: {OUT_PATH.relative_to(ROOT)}", file=sys.stderr)


if __name__ == "__main__":
    main()
