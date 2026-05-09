#!/usr/bin/env python3
"""Strict-mirror drift-guard for Yggdrasil's CI gate.

Re-runs the R274 audit logic against the live tree and reports any
production `.rs` file that lacks both:
  - an upstream `.hs` mirror (by snake_case basename match), AND
  - a `## Naming parity` docstring stanza.

In warn-only mode (default — R275 onwards), violations emit GitHub
Actions `::warning::` lines and the script exits 0. In fail-build
mode (R288 onwards), violations exit 1.

The committed `docs/strict-mirror-audit.tsv` from R274 acts as the
allowlist: rows graded `(a)`, `(c)`, or `(c-needed)` (the latter being
known-violation rows scheduled for Phase B resolution) are
allowlisted. Net-new violations beyond that allowlist trigger the
gate.

Usage:
    python3 scripts/check-strict-mirror.py
    python3 scripts/check-strict-mirror.py --fail-on-violation
    cargo parity-strict-mirror

Exit codes:
  0 - no NEW violations beyond the committed allowlist (or fail-on-violation
      is off and only allowlisted violations were found).
  1 - NEW violations found and --fail-on-violation is set.
"""

from __future__ import annotations

import argparse
import importlib.util
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
AUDIT_SCRIPT = ROOT / "scripts" / "audit-strict-mirror.py"
ALLOWLIST_TSV = ROOT / "docs" / "strict-mirror-audit.tsv"


def load_audit_module():
    """Import the audit script as a module to reuse its logic."""
    spec = importlib.util.spec_from_file_location("audit_mirror", AUDIT_SCRIPT)
    if spec is None or spec.loader is None:
        print(
            f"check-strict-mirror error: cannot load {AUDIT_SCRIPT}",
            file=sys.stderr,
        )
        raise SystemExit(2)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_allowlist() -> dict[str, str]:
    """Load the committed audit TSV; map rust_path -> final_verdict."""
    if not ALLOWLIST_TSV.exists():
        print(
            f"check-strict-mirror error: {ALLOWLIST_TSV} missing; "
            "run `python3 scripts/audit-strict-mirror.py` first.",
            file=sys.stderr,
        )
        raise SystemExit(2)
    out: dict[str, str] = {}
    for i, line in enumerate(ALLOWLIST_TSV.read_text(encoding="utf-8").splitlines()):
        if i == 0:
            continue
        parts = line.split("\t")
        if len(parts) < 7:
            continue
        rust_path, _candidates, _matched, _hits, _docstring, _initial, final = parts[:7]
        out[rust_path] = final
    return out


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Strict-mirror drift-guard (CI gate)."
    )
    parser.add_argument(
        "--fail-on-violation",
        action="store_true",
        help="exit 1 if NEW violations are found (R288+ fail-build mode).",
    )
    args = parser.parse_args()

    audit = load_audit_module()
    allowlist = load_allowlist()
    index = audit.load_index()
    rust_files = audit.iter_rust_files()

    violations: list[tuple[str, str]] = []
    for rust_path in rust_files:
        rel = rust_path.relative_to(ROOT).as_posix()
        candidates = audit.derive_candidates(rust_path.stem)
        hits: list[str] = []
        seen: set[str] = set()
        for cand in candidates:
            if cand in index:
                for p in index[cand]:
                    if p not in seen:
                        seen.add(p)
                        hits.append(p)
        parity_state = audit.has_naming_parity_block(rust_path)
        initial_verdict = "candidate_match" if hits else "no_candidate_match"
        final_verdict, _notes = audit.auto_grade(
            rel, hits, parity_state, initial_verdict
        )
        # Allowlist semantics: a row is allowlisted if its rust_path appears
        # in the committed TSV. New files (added since R274) that don't
        # auto-grade as (a) or (c) trigger a violation.
        if rel in allowlist:
            continue
        if final_verdict.startswith("(a)") or final_verdict.startswith("(c)"):
            continue
        # New file with neither upstream mirror nor docstring stanza.
        violations.append((rel, final_verdict))

    if not violations:
        print("strict-mirror: 0 violations (clean)", file=sys.stderr)
        return

    # Emit warnings in GitHub Actions format. The `::warning file=...::`
    # syntax surfaces as an annotation on the file in the PR review UI.
    print(
        f"strict-mirror: {len(violations)} new file(s) violate the policy "
        "(neither upstream `.hs` mirror nor `## Naming parity` docstring stanza):",
        file=sys.stderr,
    )
    for rel, verdict in violations:
        print(
            f"::warning file={rel}::{verdict} - "
            "must either rename to an upstream `.hs` basename or add a "
            "`## Naming parity` docstring block",
            file=sys.stderr,
        )

    if args.fail_on_violation:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
