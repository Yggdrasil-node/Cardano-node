#!/usr/bin/env python3
"""Validate docs/parity-matrix.json against the local Rust and Haskell trees.

Usage:
    python scripts/check-parity-matrix.py

The script enforces:
  - Schema version matches the expected revision.
  - Reference tag matches the pinned IntersectMBO/cardano-node release.
  - Every `haskell_reference[*].path` and `rust_surface[*].path` exists on disk.
  - Each entry's status is one of the allowed states and is consistent with
    the `implemented_evidence` and `remaining_work` lists.

Exit codes:
  0 -- matrix is clean.
  1 -- schema, status, or path validation failed.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
MATRIX_PATH = ROOT / "docs" / "parity-matrix.json"
# Yggdrasil tracks the latest IntersectMBO/cardano-node release; bump this
# constant whenever upstream ships a new tag and re-validate every
# haskell_reference.path in docs/parity-matrix.json (paths can move across
# releases).
REFERENCE_TAG = "11.0.1"

ALLOWED_STATUS = {
    "verified_11_0_1",
    "implemented_needs_11_0_1_evidence",
    "partial",
    "absent",
}
ALLOWED_AREAS = {
    "node",
    "network",
    "consensus",
    "storage",
    "ledger",
    "plutus",
    "crypto",
    "runtime",
    "mempool",
    "producer",
    "observability",
    "fixtures",
}
ALLOWED_MILESTONES = {"R266", "R267", "R268", "R272", "R273", "R274", "R275"}


def fail(message: str) -> None:
    print(f"parity-matrix error: {message}", file=sys.stderr)
    raise SystemExit(1)


def load_matrix() -> dict[str, Any]:
    try:
        return json.loads(MATRIX_PATH.read_text(encoding="utf-8"))
    except FileNotFoundError:
        fail(f"missing {MATRIX_PATH.relative_to(ROOT)}")
    except json.JSONDecodeError as exc:
        fail(f"invalid JSON at line {exc.lineno}, column {exc.colno}: {exc.msg}")


def require_string(obj: dict[str, Any], key: str, context: str) -> str:
    value = obj.get(key)
    if not isinstance(value, str) or not value.strip():
        fail(f"{context}.{key} must be a non-empty string")
    return value


def require_string_list(
    obj: dict[str, Any],
    key: str,
    context: str,
    *,
    allow_empty: bool,
) -> list[str]:
    value = obj.get(key)
    if not isinstance(value, list):
        fail(f"{context}.{key} must be a list")
    if not allow_empty and not value:
        fail(f"{context}.{key} must not be empty")
    for index, item in enumerate(value):
        if not isinstance(item, str) or not item.strip():
            fail(f"{context}.{key}[{index}] must be a non-empty string")
    return value


def validate_path_list(entry: dict[str, Any], key: str, entry_id: str) -> None:
    value = entry.get(key)
    if not isinstance(value, list) or not value:
        fail(f"{entry_id}.{key} must be a non-empty list")
    for index, item in enumerate(value):
        if not isinstance(item, dict):
            fail(f"{entry_id}.{key}[{index}] must be an object")
        rel = require_string(item, "path", f"{entry_id}.{key}[{index}]")
        require_string(item, "role", f"{entry_id}.{key}[{index}]")
        if Path(rel).is_absolute():
            fail(f"{entry_id}.{key}[{index}].path must be repository-relative")
        if ".." in Path(rel).parts:
            fail(f"{entry_id}.{key}[{index}].path must not contain '..'")
        if not (ROOT / rel).exists():
            fail(f"{entry_id}.{key}[{index}].path does not exist: {rel}")


def validate_entry(entry: dict[str, Any], seen: set[str]) -> None:
    entry_id = require_string(entry, "id", "entry")
    if entry_id in seen:
        fail(f"duplicate entry id: {entry_id}")
    seen.add(entry_id)

    area = require_string(entry, "area", entry_id)
    if area not in ALLOWED_AREAS:
        fail(f"{entry_id}.area has unsupported value: {area}")
    status = require_string(entry, "status", entry_id)
    if status not in ALLOWED_STATUS:
        fail(f"{entry_id}.status has unsupported value: {status}")
    milestone = require_string(entry, "next_milestone", entry_id)
    if milestone not in ALLOWED_MILESTONES:
        fail(f"{entry_id}.next_milestone has unsupported value: {milestone}")

    require_string(entry, "feature", entry_id)
    validate_path_list(entry, "haskell_reference", entry_id)
    validate_path_list(entry, "rust_surface", entry_id)

    evidence = require_string_list(
        entry,
        "implemented_evidence",
        entry_id,
        allow_empty=status == "absent",
    )
    remaining = require_string_list(
        entry,
        "remaining_work",
        entry_id,
        allow_empty=status == "verified_11_0_1",
    )
    require_string_list(entry, "acceptance", entry_id, allow_empty=False)

    if status == "verified_11_0_1" and not evidence:
        fail(f"{entry_id} is verified_11_0_1 but has no implemented_evidence")
    if status == "absent" and evidence:
        fail(f"{entry_id} is absent but lists implemented_evidence")
    if status != "verified_11_0_1" and not remaining:
        fail(f"{entry_id} must list remaining work unless verified_11_0_1")


def main() -> int:
    matrix = load_matrix()
    if matrix.get("schema_version") != 1:
        fail("schema_version must be 1")
    reference = matrix.get("reference")
    if not isinstance(reference, dict):
        fail("reference must be an object")
    if reference.get("tag") != REFERENCE_TAG:
        fail(f"reference.tag must be {REFERENCE_TAG}")
    local_root = require_string(reference, "local_root", "reference")
    if not (ROOT / local_root).is_dir():
        fail(
            f"reference.local_root does not exist; "
            f"run scripts/setup-reference.sh: {local_root}"
        )

    entries = matrix.get("entries")
    if not isinstance(entries, list) or not entries:
        fail("entries must be a non-empty list")

    seen: set[str] = set()
    for entry in entries:
        if not isinstance(entry, dict):
            fail("each entry must be an object")
        validate_entry(entry, seen)

    print(
        f"parity matrix clean: {len(entries)} entries validated against "
        f"{local_root} (reference tag {REFERENCE_TAG})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
