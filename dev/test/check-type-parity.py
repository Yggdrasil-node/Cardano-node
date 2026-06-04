#!/usr/bin/env python3
"""Exact-text type/name parity guard.

This checker validates the lightweight manifest in
`docs/type-parity-audit.tsv`. It does not claim to prove semantic
equivalence; it prevents declared Rust surfaces from silently drifting
away from the upstream Haskell identifiers they intentionally mirror.

Usage:
    python3 dev/test/check-type-parity.py
    python3 dev/test/check-type-parity.py --self-test
"""

from __future__ import annotations

import argparse
import csv
import re
import tempfile
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "docs" / "type-parity-audit.tsv"
RUST_PATH_PREFIX = "crates/"
RUST_PATH_SUFFIX = ".rs"
UPSTREAM_PATH_PREFIX = ".reference-haskell-cardano-node/"
UPSTREAM_PATH_SUFFIX = ".hs"
SCOPE_PATTERN = re.compile(r"[a-z0-9]+(?:-[a-z0-9]+)*")
EXPECTED_HEADER = [
    "scope",
    "rust_path",
    "rust_text",
    "upstream_path",
    "upstream_text",
    "notes",
]


@dataclass(frozen=True)
class Row:
    line_no: int
    scope: str
    rust_path: str
    rust_text: str
    upstream_path: str
    upstream_text: str
    notes: str

    def key(self) -> tuple[str, str, str, str, str]:
        return (
            self.scope,
            self.rust_path,
            self.rust_text,
            self.upstream_path,
            self.upstream_text,
        )


def load_manifest(path: Path) -> list[Row]:
    with path.open(newline="", encoding="utf-8") as f:
        reader = csv.reader(f, delimiter="\t")
        try:
            header = next(reader)
        except StopIteration as exc:
            raise ValueError(f"{path}: empty manifest") from exc
        if header != EXPECTED_HEADER:
            raise ValueError(
                f"{path}: invalid header {header!r}; expected {EXPECTED_HEADER!r}"
            )
        rows: list[Row] = []
        for line_no, parts in enumerate(reader, start=2):
            if not parts or all(not part.strip() for part in parts):
                continue
            if len(parts) != len(EXPECTED_HEADER):
                raise ValueError(
                    f"{path}:{line_no}: expected {len(EXPECTED_HEADER)} tab-separated "
                    f"fields, got {len(parts)}"
                )
            row = Row(line_no=line_no, **dict(zip(EXPECTED_HEADER, parts)))
            for field in EXPECTED_HEADER:
                if not getattr(row, field).strip():
                    raise ValueError(f"{path}:{line_no}: `{field}` must not be empty")
            for field in ["rust_text", "upstream_text"]:
                value = getattr(row, field)
                if value != value.strip() or any(ch.isspace() for ch in value):
                    raise ValueError(
                        f"{path}:{line_no}: `{field}` must be a single token"
                    )
            if SCOPE_PATTERN.fullmatch(row.scope) is None:
                raise ValueError(
                    f"{path}:{line_no}: `scope` must be lower-kebab-case: "
                    f"{row.scope}"
                )
            if row.notes != row.notes.strip():
                raise ValueError(
                    f"{path}:{line_no}: `notes` must not have leading/trailing space"
                )
            if len(row.notes.split()) < 2 or not row.notes.endswith("."):
                raise ValueError(
                    f"{path}:{line_no}: `notes` must be an explanatory sentence"
                )
            rows.append(row)
    if not rows:
        raise ValueError(f"{path}: manifest has no rows")
    return rows


def resolve_manifest_path(
    raw_path: str,
    line_no: int,
    side: str,
    *,
    enforce_side_roots: bool = True,
) -> Path:
    path = Path(raw_path)
    if path.is_absolute():
        raise ValueError(f"line {line_no}: {side} path must be relative: {raw_path}")
    if ".." in path.parts:
        raise ValueError(f"line {line_no}: {side} path must not escape root: {raw_path}")
    if side == "rust" and path.suffix != RUST_PATH_SUFFIX:
        raise ValueError(
            f"line {line_no}: rust path must end with {RUST_PATH_SUFFIX}: {raw_path}"
        )
    if side == "upstream" and path.suffix != UPSTREAM_PATH_SUFFIX:
        raise ValueError(
            f"line {line_no}: upstream path must end with "
            f"{UPSTREAM_PATH_SUFFIX}: {raw_path}"
        )
    if (
        enforce_side_roots
        and side == "rust"
        and not raw_path.startswith(RUST_PATH_PREFIX)
    ):
        raise ValueError(
            f"line {line_no}: rust path must live under {RUST_PATH_PREFIX}: {raw_path}"
        )
    if (
        enforce_side_roots
        and side == "upstream"
        and not raw_path.startswith(UPSTREAM_PATH_PREFIX)
    ):
        raise ValueError(
            f"line {line_no}: upstream path must live under "
            f"{UPSTREAM_PATH_PREFIX}: {raw_path}"
        )
    return ROOT / path


def has_token_boundary_match(text: str, needle: str) -> bool:
    pattern = re.compile(
        r"(?<![A-Za-z0-9_])" + re.escape(needle) + r"(?![A-Za-z0-9_])"
    )
    return pattern.search(text) is not None


def ensure_contains(path: Path, needle: str, line_no: int, side: str) -> str | None:
    if not path.exists():
        return f"line {line_no}: {side} file does not exist: {path.relative_to(ROOT)}"
    text = path.read_text(encoding="utf-8")
    if not has_token_boundary_match(text, needle):
        return (
            f"line {line_no}: {side} token {needle!r} not found in "
            f"{path.relative_to(ROOT)} with identifier boundaries"
        )
    return None


def check_manifest(path: Path, *, enforce_side_roots: bool = True) -> list[str]:
    errors: list[str] = []
    try:
        rows = load_manifest(path)
    except ValueError as exc:
        return [str(exc)]
    for previous, row in zip(rows, rows[1:]):
        if previous.key() > row.key():
            errors.append(
                f"line {row.line_no}: manifest rows must be sorted by "
                "scope, rust_path, rust_text, upstream_path, upstream_text"
            )
            break
    seen: dict[tuple[str, str, str, str, str], int] = {}
    for row in rows:
        key = row.key()
        if key in seen:
            errors.append(
                f"line {row.line_no}: duplicate mapping; first seen on line {seen[key]}"
            )
            continue
        seen[key] = row.line_no
        try:
            rust_path = resolve_manifest_path(
                row.rust_path,
                row.line_no,
                "rust",
                enforce_side_roots=enforce_side_roots,
            )
            upstream_path = resolve_manifest_path(
                row.upstream_path,
                row.line_no,
                "upstream",
                enforce_side_roots=enforce_side_roots,
            )
        except ValueError as exc:
            errors.append(str(exc))
            continue
        errors.extend(
            error
            for error in [
                ensure_contains(rust_path, row.rust_text, row.line_no, "rust"),
                ensure_contains(
                    upstream_path,
                    row.upstream_text,
                    row.line_no,
                    "upstream",
                ),
            ]
            if error is not None
        )
    return errors


def run_self_test() -> None:
    tmp_parent = ROOT / "target"
    tmp_parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="type-parity-self-test-", dir=tmp_parent) as td:
        base = Path(td)
        rust = base / "rust.rs"
        upstream = base / "Upstream.hs"
        rust.write_text("pub enum Example { Dijkstra }\n", encoding="utf-8")
        upstream.write_text("data Example = ShelleyBasedEraDijkstra\n", encoding="utf-8")
        manifest = base / "manifest.tsv"
        manifest.write_text(
            "\t".join(EXPECTED_HEADER)
            + "\n"
            + "\t".join(
                [
                    "self-test",
                    str(rust.relative_to(ROOT)),
                    "Dijkstra",
                    str(upstream.relative_to(ROOT)),
                    "ShelleyBasedEraDijkstra",
                    "Valid self-test mapping.",
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        rows = load_manifest(manifest)
        assert rows[0].rust_text == "Dijkstra"
        assert check_manifest(manifest, enforce_side_roots=False) == []

        duplicate_manifest = base / "duplicate.tsv"
        duplicate_manifest.write_text(
            manifest.read_text(encoding="utf-8")
            + "\t".join(
                [
                    "self-test",
                    str(rust.relative_to(ROOT)),
                    "Dijkstra",
                    str(upstream.relative_to(ROOT)),
                    "ShelleyBasedEraDijkstra",
                    "Duplicate self-test mapping.",
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        assert any(
            "duplicate mapping" in err for err in check_manifest(duplicate_manifest)
        )

        escaping_manifest = base / "escaping.tsv"
        escaping_manifest.write_text(
            "\t".join(EXPECTED_HEADER)
            + "\n"
            + "\t".join(
                [
                    "self-test",
                    "../outside.rs",
                    "Dijkstra",
                    str(upstream.relative_to(ROOT)),
                    "ShelleyBasedEraDijkstra",
                    "Escaping path rejection.",
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        assert any("must not escape root" in err for err in check_manifest(escaping_manifest))

        bad_scope_manifest = base / "bad-scope.tsv"
        bad_scope_manifest.write_text(
            "\t".join(EXPECTED_HEADER)
            + "\n"
            + "\t".join(
                [
                    "Bad Scope",
                    str(rust.relative_to(ROOT)),
                    "Dijkstra",
                    str(upstream.relative_to(ROOT)),
                    "ShelleyBasedEraDijkstra",
                    "Invalid scope rejection.",
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        assert any(
            "`scope` must be lower-kebab-case" in err
            for err in check_manifest(bad_scope_manifest)
        )

        bad_notes_manifest = base / "bad-notes.tsv"
        bad_notes_manifest.write_text(
            "\t".join(EXPECTED_HEADER)
            + "\n"
            + "\t".join(
                [
                    "self-test",
                    str(rust.relative_to(ROOT)),
                    "Dijkstra",
                    str(upstream.relative_to(ROOT)),
                    "ShelleyBasedEraDijkstra",
                    "todo",
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        assert any(
            "`notes` must be an explanatory sentence" in err
            for err in check_manifest(bad_notes_manifest)
        )

        bad_text_manifest = base / "bad-text.tsv"
        bad_text_manifest.write_text(
            "\t".join(EXPECTED_HEADER)
            + "\n"
            + "\t".join(
                [
                    "self-test",
                    str(rust.relative_to(ROOT)),
                    "Bad Token",
                    str(upstream.relative_to(ROOT)),
                    "ShelleyBasedEraDijkstra",
                    "Invalid token rejection.",
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        assert any(
            "`rust_text` must be a single token" in err
            for err in check_manifest(bad_text_manifest)
        )

        partial_match_manifest = base / "partial-match.tsv"
        partial_match_manifest.write_text(
            "\t".join(EXPECTED_HEADER)
            + "\n"
            + "\t".join(
                [
                    "self-test",
                    str(rust.relative_to(ROOT)),
                    "Era",
                    str(upstream.relative_to(ROOT)),
                    "BasedEra",
                    "Partial token rejection.",
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        partial_match_errors = check_manifest(
            partial_match_manifest,
            enforce_side_roots=False,
        )
        assert any("rust token 'Era' not found" in err for err in partial_match_errors)
        assert any(
            "upstream token 'BasedEra' not found" in err for err in partial_match_errors
        )

        unsorted_manifest = base / "unsorted.tsv"
        unsorted_manifest.write_text(
            "\t".join(EXPECTED_HEADER)
            + "\n"
            + "\t".join(
                [
                    "self-test",
                    str(rust.relative_to(ROOT)),
                    "Zed",
                    str(upstream.relative_to(ROOT)),
                    "Zed",
                    "Unsorted first mapping.",
                ]
            )
            + "\n"
            + "\t".join(
                [
                    "self-test",
                    str(rust.relative_to(ROOT)),
                    "Alpha",
                    str(upstream.relative_to(ROOT)),
                    "Alpha",
                    "Unsorted second mapping.",
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        assert any(
            "manifest rows must be sorted" in err
            for err in check_manifest(unsorted_manifest, enforce_side_roots=False)
        )

        bad_rust_root_manifest = base / "bad-rust-root.tsv"
        bad_rust_root_manifest.write_text(
            "\t".join(EXPECTED_HEADER)
            + "\n"
            + "\t".join(
                [
                    "self-test",
                    str(rust.relative_to(ROOT)),
                    "Dijkstra",
                    str(upstream.relative_to(ROOT)),
                    "ShelleyBasedEraDijkstra",
                    "Invalid Rust root rejection.",
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        assert any(
            "rust path must live under crates/" in err
            for err in check_manifest(bad_rust_root_manifest)
        )

        bad_upstream_root_manifest = base / "bad-upstream-root.tsv"
        bad_upstream_root_manifest.write_text(
            "\t".join(EXPECTED_HEADER)
            + "\n"
            + "\t".join(
                [
                    "self-test",
                    "crates/tools/cardano-testnet/src/types.rs",
                    "CardanoEra",
                    str(upstream.relative_to(ROOT)),
                    "ShelleyBasedEraDijkstra",
                    "Invalid upstream root rejection.",
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        assert any(
            "upstream path must live under .reference-haskell-cardano-node/" in err
            for err in check_manifest(bad_upstream_root_manifest)
        )

        bad_rust_suffix_manifest = base / "bad-rust-suffix.tsv"
        bad_rust_suffix_manifest.write_text(
            "\t".join(EXPECTED_HEADER)
            + "\n"
            + "\t".join(
                [
                    "self-test",
                    "crates/tools/cardano-testnet/README.md",
                    "CardanoEra",
                    str(upstream.relative_to(ROOT)),
                    "ShelleyBasedEraDijkstra",
                    "Invalid Rust suffix rejection.",
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        assert any(
            "rust path must end with .rs" in err
            for err in check_manifest(bad_rust_suffix_manifest)
        )

        bad_upstream_suffix_manifest = base / "bad-upstream-suffix.tsv"
        bad_upstream_suffix_manifest.write_text(
            "\t".join(EXPECTED_HEADER)
            + "\n"
            + "\t".join(
                [
                    "self-test",
                    "crates/tools/cardano-testnet/src/types.rs",
                    "CardanoEra",
                    ".reference-haskell-cardano-node/README.md",
                    "ShelleyBasedEraDijkstra",
                    "Invalid upstream suffix rejection.",
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        assert any(
            "upstream path must end with .hs" in err
            for err in check_manifest(bad_upstream_suffix_manifest)
        )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        run_self_test()
        print("type-parity self-test clean")
        return

    errors = check_manifest(MANIFEST)
    if errors:
        print("type-parity: violations found")
        for error in errors:
            print(f"- {error}")
        raise SystemExit(1)
    print(f"type-parity: {len(load_manifest(MANIFEST))} rows clean")


if __name__ == "__main__":
    main()
