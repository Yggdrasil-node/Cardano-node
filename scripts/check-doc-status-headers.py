#!/usr/bin/env python3
"""Validate canonical status headers across key parity documents."""
from __future__ import annotations

import json
import re
import sys
import tempfile
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MATRIX_PATH = ROOT / "docs" / "parity-matrix.json"
DASHBOARD_PATH = ROOT / "docs" / "PARITY_DASHBOARD.md"
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
DOCUMENT_CLASSIFICATION_RE = re.compile(
    r"^(?:>\s*)?\*\*Document classification:\*\*\s*(current|operational|historical)\.?\s*$",
    re.MULTILINE,
)
ROUND_RE = re.compile(r"^R([0-9]+)$")
DASHBOARD_UPDATED_RE = re.compile(r"^_Last updated:\s*([0-9]{4}-[0-9]{2}-[0-9]{2})\._\s*$", re.MULTILINE)
CANONICAL_STATUS_HEADER_RE = re.compile(r"^## Canonical Status Header[ \t]*$", re.MULTILINE)


@dataclass
class Header:
    classification: str
    fields: dict[str, str]


def parse_header(path: Path) -> Header:
    text = path.read_text(encoding="utf-8")
    banners = list(DOCUMENT_CLASSIFICATION_RE.finditer(text))
    if not banners:
        raise ValueError("missing document classification banner")
    if len(banners) > 1:
        raise ValueError("duplicate document classification banners")
    classification = banners[0].group(1)

    markers = list(CANONICAL_STATUS_HEADER_RE.finditer(text))
    if not markers:
        raise ValueError("missing '## Canonical Status Header' section")
    if len(markers) > 1:
        raise ValueError("duplicate '## Canonical Status Header' sections")
    section = text[markers[0].end():]
    fields: dict[str, str] = {}
    for line in section.splitlines():
        m = re.match(r"- \*\*(.+?):\*\*\s*(.+?)\s*$", line)
        if not m:
            if fields and line.strip() == "":
                break
            continue
        key = m.group(1)
        if key in fields:
            raise ValueError(f"duplicate header field: {key}")
        fields[key] = m.group(2)

    missing = [f for f in REQUIRED_FIELDS if f not in fields]
    if missing:
        raise ValueError(f"missing required fields: {', '.join(missing)}")
    return Header(classification=classification, fields=fields)


def parse_round(value: str) -> int:
    match = ROUND_RE.match(value)
    if not match:
        raise ValueError(f"round ceiling must be formatted as R<number>, got '{value}'")
    return int(match.group(1))


def latest_operational_round() -> int:
    latest = 0
    for path in (ROOT / "docs" / "operational-runs").glob("*round-*.md"):
        if not path.is_file():
            continue
        match = re.search(r"round-([0-9]+)", path.name)
        if match:
            latest = max(latest, int(match.group(1)))
    return latest


def parity_matrix_summary() -> tuple[str, int, Counter[str]]:
    try:
        data = json.loads(MATRIX_PATH.read_text(encoding="utf-8"))
    except FileNotFoundError:
        raise ValueError("missing docs/parity-matrix.json")
    except json.JSONDecodeError as exc:
        raise ValueError(f"invalid docs/parity-matrix.json at line {exc.lineno}: {exc.msg}") from exc

    reference = data.get("reference")
    if not isinstance(reference, dict):
        raise ValueError("docs/parity-matrix.json.reference must be an object")
    tag = reference.get("tag")
    if not isinstance(tag, str) or not tag.strip():
        raise ValueError("docs/parity-matrix.json.reference.tag must be a non-empty string")

    entries = data.get("entries")
    if not isinstance(entries, list):
        raise ValueError("docs/parity-matrix.json.entries must be an array")
    statuses: list[str] = []
    for idx, entry in enumerate(entries):
        if not isinstance(entry, dict):
            raise ValueError(f"docs/parity-matrix.json.entries[{idx}] must be an object")
        status = entry.get("status")
        if not isinstance(status, str) or not status:
            raise ValueError(f"docs/parity-matrix.json.entries[{idx}].status must be a non-empty string")
        statuses.append(status)
    return tag, len(entries), Counter(statuses)


def markdown_table_cells(line: str) -> list[str] | None:
    stripped = line.strip()
    if not stripped.startswith("|") or not stripped.endswith("|"):
        return None
    return [cell.strip() for cell in stripped.strip("|").split("|")]


def is_separator_row(cells: list[str]) -> bool:
    return bool(cells) and all(cell and "-" in cell and set(cell) <= {"-", ":", " "} for cell in cells)


def dashboard_metric_rows(text: str) -> list[list[str]]:
    lines = text.splitlines()
    summary_rows: list[list[str]] | None = None
    for idx, line in enumerate(lines):
        cells = markdown_table_cells(line)
        if cells != ["Metric", "Current value", "Detail link"]:
            continue

        if summary_rows is not None:
            raise ValueError("docs/PARITY_DASHBOARD.md: duplicate metric summary table")

        if idx + 1 >= len(lines):
            raise ValueError("docs/PARITY_DASHBOARD.md: malformed metric summary table")
        separator = markdown_table_cells(lines[idx + 1])
        if separator is None or not is_separator_row(separator):
            raise ValueError("docs/PARITY_DASHBOARD.md: malformed metric summary table")

        rows: list[list[str]] = []
        for row_line in lines[idx + 2:]:
            row = markdown_table_cells(row_line)
            if row is None:
                break
            rows.append(row)
        summary_rows = rows

    if summary_rows is None:
        raise ValueError("docs/PARITY_DASHBOARD.md: missing metric summary table")
    return summary_rows


def dashboard_summary() -> tuple[str, int, Counter[str]]:
    try:
        text = DASHBOARD_PATH.read_text(encoding="utf-8")
    except FileNotFoundError:
        raise ValueError("missing docs/PARITY_DASHBOARD.md")

    updated_matches = DASHBOARD_UPDATED_RE.findall(text)
    if not updated_matches:
        raise ValueError("docs/PARITY_DASHBOARD.md: missing '_Last updated: YYYY-MM-DD._' line")
    if len(updated_matches) > 1:
        raise ValueError("docs/PARITY_DASHBOARD.md: duplicate '_Last updated: YYYY-MM-DD._' lines")

    rows = dashboard_metric_rows(text)

    total_matches: list[str] = []
    status_rows: list[tuple[str, str]] = []
    for row in rows:
        if len(row) < 2:
            continue
        label = row[0]
        value = row[1]
        if label == "Total parity entries":
            if not value.isdecimal():
                raise ValueError("docs/PARITY_DASHBOARD.md: 'Total parity entries' value must be numeric")
            total_matches.append(value)
        elif label.startswith("`") and label.endswith("`") and len(label) > 2:
            if not value.isdecimal():
                raise ValueError(f"docs/PARITY_DASHBOARD.md: status-count value for {label} must be numeric")
            status_rows.append((label[1:-1], value))

    if not total_matches:
        raise ValueError("docs/PARITY_DASHBOARD.md: missing 'Total parity entries' table row")
    if len(total_matches) > 1:
        raise ValueError("docs/PARITY_DASHBOARD.md: duplicate 'Total parity entries' table rows")

    if not status_rows:
        raise ValueError("docs/PARITY_DASHBOARD.md: missing status-count table rows")
    status_row_counts = Counter(status for status, _ in status_rows)
    duplicate_statuses = sorted(
        status for status, count in status_row_counts.items() if count > 1
    )
    if duplicate_statuses:
        raise ValueError(
            "docs/PARITY_DASHBOARD.md: duplicate status-count rows: "
            + ", ".join(duplicate_statuses)
        )

    counts: Counter[str] = Counter()
    for status, count in status_rows:
        counts[status] = int(count)

    return updated_matches[0], int(total_matches[0]), counts


def assert_raises_value_error(label: str, needle: str, func) -> None:
    try:
        func()
    except ValueError as exc:
        if needle not in str(exc):
            raise AssertionError(
                f"{label}: expected error containing {needle!r}, got {exc!r}"
            ) from exc
    else:
        raise AssertionError(f"{label}: expected ValueError containing {needle!r}")


def self_test() -> int:
    global ROOT, DASHBOARD_PATH, MATRIX_PATH

    errors: list[str] = []
    original_root = ROOT
    original_dashboard_path = DASHBOARD_PATH
    original_matrix_path = MATRIX_PATH
    canonical_field_lines = [
        "- **As of date:** 2026-05-26",
        "- **Round ceiling:** R839",
        "- **Parity tag:** 11.0.1",
        "- **Test baseline date:** 2026-05-26",
        "- **Source of truth:** docs/parity-matrix.json",
    ]
    dashboard_row_lines = [
        "| Total parity entries | 22 | link |",
        "| `verified_11_0_1` | 2 | link |",
        "| `implemented_needs_11_0_1_evidence` | 12 | link |",
        "| `partial` | 8 | link |",
    ]

    def record(name: str, func) -> None:
        try:
            func()
        except AssertionError as exc:
            errors.append(f"{name}: {exc}")

    def status_header(field_lines: list[str] | None = None) -> str:
        lines = canonical_field_lines if field_lines is None else field_lines
        return (
            "## Canonical Status Header\n\n"
            + "\n".join(lines)
            + "\n\n"
        )

    def status_body(
        classification_line: str = "**Document classification:** current",
        field_lines: list[str] | None = None,
        suffix: str = "",
    ) -> str:
        return classification_line + "\n\n" + status_header(field_lines) + suffix

    def with_status_doc(body: str, assertion) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "PARITY_SUMMARY.md"
            path.write_text(body, encoding="utf-8")
            assertion(path)

    def dashboard_table(row_lines: list[str] | None = None) -> str:
        rows = dashboard_row_lines if row_lines is None else row_lines
        return (
            "| Metric | Current value | Detail link |\n"
            "| --- | ---: | --- |\n"
            + "\n".join(rows)
            + "\n"
        )

    def dashboard_body(extra: str = "", row_lines: list[str] | None = None) -> str:
        return (
            "# Parity Dashboard\n\n_Last updated: 2026-05-26._\n\n"
            + dashboard_table(row_lines)
            + extra
        )

    def latest_round_ignores_non_markdown_artifacts() -> None:
        global ROOT
        previous_root = ROOT
        with tempfile.TemporaryDirectory() as tmp:
            try:
                root = Path(tmp)
                runs = root / "docs" / "operational-runs"
                runs.mkdir(parents=True)
                (runs / "2026-05-26-round-824-good.md").write_text("# record\n", encoding="utf-8")
                (runs / "2026-05-26-round-999-artifact.log").write_text("artifact\n", encoding="utf-8")
                (runs / "2026-05-26-round-1000-artifact.txt").write_text("artifact\n", encoding="utf-8")
                ROOT = root
                actual = latest_operational_round()
                if actual != 824:
                    raise AssertionError(f"expected latest markdown round 824, got {actual}")
            finally:
                ROOT = previous_root

    def latest_round_fixture_restores_root_after_scan() -> None:
        before = ROOT
        latest_round_ignores_non_markdown_artifacts()
        if ROOT != before:
            raise AssertionError("latest-round fixture leaked ROOT after scan")

    def duplicate_header_fields_are_rejected() -> None:
        body = status_body(
            field_lines=[
                "- **As of date:** 2026-05-26",
                "- **As of date:** 2026-05-27",
                *canonical_field_lines[1:],
            ]
        )
        with_status_doc(
            body,
            lambda path: assert_raises_value_error(
                "duplicate header field", "duplicate header field", lambda: parse_header(path)
            ),
        )

    def duplicate_header_sections_are_rejected() -> None:
        header = status_header()
        body = "**Document classification:** current\n\n" + header + "body text\n\n" + header
        with_status_doc(
            body,
            lambda path: assert_raises_value_error(
                "duplicate header sections",
                "duplicate '## Canonical Status Header' sections",
                lambda: parse_header(path),
            ),
        )

    def prose_status_header_mentions_are_allowed() -> None:
        body = status_body(
            suffix="This prose mention of ## Canonical Status Header is not a second heading.\n\n"
        )

        def assertion(path: Path) -> None:
            header = parse_header(path)
            if header.fields["Round ceiling"] != "R839":
                raise AssertionError("expected prose marker mention to preserve parsed fields")

        with_status_doc(body, assertion)

    def inline_classification_mentions_are_ignored() -> None:
        body = (
            "# Missing Banner\n\n"
            "This prose mentions **Document classification:** current but is not the banner line.\n\n"
            + status_header()
        )
        with_status_doc(
            body,
            lambda path: assert_raises_value_error(
                "inline classification mention",
                "missing document classification banner",
                lambda: parse_header(path),
            ),
        )

    def blockquote_classification_banner_is_accepted() -> None:
        body = status_body("> **Document classification:** current.")

        def assertion(path: Path) -> None:
            header = parse_header(path)
            if header.classification != "current":
                raise AssertionError(f"expected current classification, got {header.classification}")

        with_status_doc(body, assertion)

    def duplicate_classification_banners_are_rejected() -> None:
        body = status_body("**Document classification:** current\n**Document classification:** operational")
        with_status_doc(
            body,
            lambda path: assert_raises_value_error(
                "duplicate classification banners",
                "duplicate document classification banners",
                lambda: parse_header(path),
            ),
        )

    def with_dashboard(body: str, assertion) -> None:
        global DASHBOARD_PATH
        previous_dashboard_path = DASHBOARD_PATH
        with tempfile.TemporaryDirectory() as tmp:
            try:
                DASHBOARD_PATH = Path(tmp) / "PARITY_DASHBOARD.md"
                DASHBOARD_PATH.write_text(body, encoding="utf-8")
                assertion()
            finally:
                DASHBOARD_PATH = previous_dashboard_path

    def dashboard_rejects_duplicate_date_rows() -> None:
        body = dashboard_body().replace(
            "_Last updated: 2026-05-26._",
            "_Last updated: 2026-05-26._\n_Last updated: 2026-05-27._",
        )
        with_dashboard(
            body,
            lambda: assert_raises_value_error("duplicate dashboard date", "duplicate", dashboard_summary),
        )

    def dashboard_rejects_duplicate_total_rows() -> None:
        body = dashboard_body(
            row_lines=[
                "| Total parity entries | 22 | link |",
                "| Total parity entries | 23 | link |",
                *dashboard_row_lines[1:],
            ]
        )
        with_dashboard(
            body,
            lambda: assert_raises_value_error("duplicate dashboard total", "duplicate", dashboard_summary),
        )

    def dashboard_rejects_duplicate_status_rows() -> None:
        with_dashboard(
            dashboard_body("| `partial` | 7 | link |\n"),
            lambda: assert_raises_value_error(
                "duplicate dashboard status",
                "duplicate status-count rows",
                dashboard_summary,
            ),
        )

    def dashboard_ignores_non_summary_tables() -> None:
        body = dashboard_body(
            """
## Notes

| Example | Count | Detail |
| --- | ---: | --- |
| `partial` | 999 | This example is outside the summary table. |
"""
        )

        def assertion() -> None:
            date, total, counts = dashboard_summary()
            if date != "2026-05-26":
                raise AssertionError(f"expected dashboard date 2026-05-26, got {date}")
            if total != 22:
                raise AssertionError(f"expected dashboard total 22, got {total}")
            expected = Counter(
                {
                    "verified_11_0_1": 2,
                    "implemented_needs_11_0_1_evidence": 12,
                    "partial": 8,
                }
            )
            if counts != expected:
                raise AssertionError(
                    f"expected summary-table status counts {dict(expected)}, got {dict(counts)}"
                )

        with_dashboard(body, assertion)

    def dashboard_rejects_duplicate_metric_summary_tables() -> None:
        duplicate_table = dashboard_table(["| Total parity entries | 22 | link |"])
        body = dashboard_body("\n## Duplicate Summary\n\n" + duplicate_table)
        with_dashboard(
            body,
            lambda: assert_raises_value_error(
                "duplicate metric summary table",
                "duplicate metric summary table",
                dashboard_summary,
            ),
        )

    def dashboard_ignores_inline_date_examples() -> None:
        body = dashboard_body(
            """
## Notes

Example text may mention `_Last updated: 2026-05-25._` without being the dashboard status date.
"""
        )

        def assertion() -> None:
            date, total, counts = dashboard_summary()
            if date != "2026-05-26":
                raise AssertionError(f"expected dashboard date 2026-05-26, got {date}")
            if total != 22:
                raise AssertionError(f"expected dashboard total 22, got {total}")
            if counts["partial"] != 8:
                raise AssertionError(f"expected partial count 8, got {counts['partial']}")

        with_dashboard(body, assertion)

    def dashboard_helper_restores_path_after_failed_assertion() -> None:
        before = DASHBOARD_PATH

        def assertion() -> None:
            raise AssertionError("sentinel dashboard assertion failure")

        try:
            with_dashboard(dashboard_body(), assertion)
        except AssertionError as exc:
            if str(exc) != "sentinel dashboard assertion failure":
                raise
        else:
            raise AssertionError("expected dashboard helper assertion failure")

        if DASHBOARD_PATH != before:
            raise AssertionError("dashboard helper leaked DASHBOARD_PATH after assertion failure")

    try:
        record(
            "latest round ignores non-markdown artifacts and restores root",
            latest_round_fixture_restores_root_after_scan,
        )
        record("duplicate header fields are rejected", duplicate_header_fields_are_rejected)
        record("duplicate header sections are rejected", duplicate_header_sections_are_rejected)
        record("prose status-header mentions are allowed", prose_status_header_mentions_are_allowed)
        record("inline classification mentions are ignored", inline_classification_mentions_are_ignored)
        record("blockquote classification banner is accepted", blockquote_classification_banner_is_accepted)
        record("duplicate classification banners are rejected", duplicate_classification_banners_are_rejected)
        record("dashboard rejects duplicate date rows", dashboard_rejects_duplicate_date_rows)
        record("dashboard rejects duplicate total rows", dashboard_rejects_duplicate_total_rows)
        record("dashboard rejects duplicate status rows", dashboard_rejects_duplicate_status_rows)
        record("dashboard ignores non-summary tables", dashboard_ignores_non_summary_tables)
        record("dashboard rejects duplicate metric summary tables", dashboard_rejects_duplicate_metric_summary_tables)
        record("dashboard ignores inline date examples", dashboard_ignores_inline_date_examples)
        record(
            "dashboard helper restores path after failed assertion",
            dashboard_helper_restores_path_after_failed_assertion,
        )
    finally:
        ROOT = original_root
        DASHBOARD_PATH = original_dashboard_path
        MATRIX_PATH = original_matrix_path

    if errors:
        for error in errors:
            print(f"ERROR: {error}")
        return 1

    print("doc-status self-test clean")
    return 0


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
        for key in ("As of date", "Round ceiling", "Parity tag", "Test baseline date"):
            if header.fields[key] != reference[key]:
                errors.append(
                    f"{rel}: {key}='{header.fields[key]}' disagrees with docs/PARITY_SUMMARY.md '{reference[key]}'"
                )

    try:
        declared_round = parse_round(reference["Round ceiling"])
    except ValueError as exc:
        errors.append(f"docs/PARITY_SUMMARY.md: {exc}")
        declared_round = 0

    latest_round = latest_operational_round()
    if latest_round and declared_round < latest_round:
        errors.append(
            "docs/PARITY_SUMMARY.md: Round ceiling="
            f"'{reference['Round ceiling']}' is behind latest operational run R{latest_round}"
        )

    try:
        matrix_tag, matrix_total, matrix_counts = parity_matrix_summary()
    except ValueError as exc:
        errors.append(str(exc))
    else:
        if reference["Parity tag"] != matrix_tag:
            errors.append(
                "docs/PARITY_SUMMARY.md: Parity tag="
                f"'{reference['Parity tag']}' disagrees with docs/parity-matrix.json reference.tag '{matrix_tag}'"
            )
        try:
            dashboard_date, dashboard_total, dashboard_counts = dashboard_summary()
        except ValueError as exc:
            errors.append(str(exc))
        else:
            if dashboard_date != reference["As of date"]:
                errors.append(
                    "docs/PARITY_DASHBOARD.md: Last updated="
                    f"'{dashboard_date}' disagrees with docs/PARITY_SUMMARY.md As of date '{reference['As of date']}'"
                )
            if dashboard_total != matrix_total:
                errors.append(
                    "docs/PARITY_DASHBOARD.md: Total parity entries="
                    f"{dashboard_total} disagrees with docs/parity-matrix.json entries {matrix_total}"
                )
            if dashboard_counts != matrix_counts:
                errors.append(
                    "docs/PARITY_DASHBOARD.md: status counts="
                    f"{dict(sorted(dashboard_counts.items()))} disagree with docs/parity-matrix.json "
                    f"{dict(sorted(matrix_counts.items()))}"
                )

    if errors:
        for err in errors:
            print(f"ERROR: {err}")
        return 1

    print("OK: Canonical status headers are aligned.")
    return 0


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        sys.exit(self_test())
    if len(sys.argv) > 1:
        print("usage: check-doc-status-headers.py [--self-test]", file=sys.stderr)
        sys.exit(2)
    sys.exit(main())
