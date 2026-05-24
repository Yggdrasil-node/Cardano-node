#!/usr/bin/env python3
"""Validate canonical status headers across key parity documents."""
from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCS = {
    "docs/PARITY_SUMMARY.md": "current",
    "docs/UPSTREAM_PARITY.md": "operational",
    "docs/COMPLETION_ROADMAP.md": "operational",
}
REQUIRED_FIELDS = [
    "As of date",
    "Round ceiling",
    "Parity tag",
    "Test baseline date",
    "Source of truth",
]


@dataclass
class Header:
    classification: str
    fields: dict[str, str]


def parse_header(path: Path) -> Header:
    text = path.read_text(encoding="utf-8")
    banner = re.search(r"\*\*Document classification:\*\*\s*(current|operational|historical)", text)
    if not banner:
        raise ValueError("missing document classification banner")
    classification = banner.group(1)

    marker = "## Canonical Status Header"
    if marker not in text:
        raise ValueError("missing '## Canonical Status Header' section")
    section = text.split(marker, 1)[1]
    fields: dict[str, str] = {}
    for line in section.splitlines():
        m = re.match(r"- \*\*(.+?):\*\*\s*(.+?)\s*$", line)
        if not m:
            if fields and line.strip() == "":
                break
            continue
        fields[m.group(1)] = m.group(2)

    missing = [f for f in REQUIRED_FIELDS if f not in fields]
    if missing:
        raise ValueError(f"missing required fields: {', '.join(missing)}")
    return Header(classification=classification, fields=fields)


def main() -> int:
    parsed: dict[str, Header] = {}
    errors: list[str] = []

    for rel, expected_class in DOCS.items():
        path = ROOT / rel
        try:
            header = parse_header(path)
            parsed[rel] = header
        except Exception as exc:
            errors.append(f"{rel}: {exc}")
            continue

        if header.classification != expected_class:
            errors.append(
                f"{rel}: classification '{header.classification}' does not match expected '{expected_class}'"
            )

    if errors:
        for err in errors:
            print(f"ERROR: {err}")
        return 1

    reference = parsed["docs/PARITY_SUMMARY.md"].fields
    for rel, header in parsed.items():
        if header.classification == "historical":
            continue
        for key in ("Round ceiling", "Parity tag", "Test baseline date"):
            if header.fields[key] != reference[key]:
                errors.append(
                    f"{rel}: {key}='{header.fields[key]}' disagrees with docs/PARITY_SUMMARY.md '{reference[key]}'"
                )

    if errors:
        for err in errors:
            print(f"ERROR: {err}")
        return 1

    print("OK: Canonical status headers are aligned.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
