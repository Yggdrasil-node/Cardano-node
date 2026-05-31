#!/usr/bin/env python3
"""Compare Gap BP CEK accumulated-step flush traces.

Rust emits one line per accumulated CEK step-budget spend when
`YGG_DUMP_CEK_FLUSHES=1`. A Haskell reference capture can be transformed into
the same key-value shape, then this helper compares the flush sequence,
per-step-kind counters, budget deltas, before/after budget, and status by
ordinal index.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any


KEY_RE = re.compile(r"([A-Za-z_][A-Za-z0-9_]*)=")
REQUIRED_KEYS = (
    "steps",
    "counts",
    "cpu",
    "mem",
    "before_cpu",
    "before_mem",
    "after_cpu",
    "after_mem",
    "status",
)
DEFAULT_COMPARE_KEYS = REQUIRED_KEYS


@dataclass(frozen=True)
class Flush:
    label: str
    source: str
    line_number: int
    ordinal: int
    fields: dict[str, str]
    counts: dict[str, int]

    def summary(self) -> dict[str, Any]:
        return {
            "label": self.label,
            "source": self.source,
            "line_number": self.line_number,
            "ordinal": self.ordinal,
            "fields": self.fields,
            "counts": self.counts,
        }


def parse_key_values(line: str) -> dict[str, str] | None:
    if "steps=" not in line or "counts=[" not in line:
        return None
    matches = list(KEY_RE.finditer(line))
    if not matches:
        return None

    fields: dict[str, str] = {}
    for index, match in enumerate(matches):
        key = match.group(1)
        value_start = match.end()
        value_end = matches[index + 1].start() if index + 1 < len(matches) else len(line)
        fields[key] = line[value_start:value_end].strip()
    return fields


def parse_counts(value: str, label: str) -> dict[str, int]:
    if not value.startswith("[") or not value.endswith("]"):
        raise SystemExit(f"{label}: counts must be bracketed")
    body = value[1:-1]
    counts: dict[str, int] = {}
    if not body:
        return counts
    for entry in body.split(","):
        if ":" not in entry:
            raise SystemExit(f"{label}: invalid counts entry {entry!r}")
        kind, raw_count = entry.split(":", 1)
        try:
            counts[kind] = int(raw_count)
        except ValueError as exc:
            raise SystemExit(f"{label}: invalid count for {kind}: {raw_count!r}") from exc
    return counts


def validate_required_keys(label: str, line_number: int, fields: dict[str, str]) -> None:
    missing = [key for key in REQUIRED_KEYS if key not in fields]
    if missing:
        joined = ", ".join(missing)
        raise SystemExit(f"{label}: line {line_number}: missing required keys: {joined}")


def load_flushes(label: str, path: Path) -> list[Flush]:
    flushes: list[Flush] = []
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        fields = parse_key_values(line)
        if fields is None:
            continue
        validate_required_keys(str(path), line_number, fields)
        counts = parse_counts(fields["counts"], f"{label}:{line_number}")
        flushes.append(
            Flush(
                label=label,
                source=str(path),
                line_number=line_number,
                ordinal=len(flushes),
                fields=fields,
                counts=counts,
            )
        )
    if not flushes:
        raise SystemExit(f"{label}: no CEK flush lines found in {path}")
    return flushes


def compare_flushes(
    rust_flushes: list[Flush],
    haskell_flushes: list[Flush] | None,
    keys: tuple[str, ...],
) -> tuple[list[dict[str, Any]], bool]:
    if haskell_flushes is None:
        return (
            [
                {
                    "ordinal": flush.ordinal,
                    "status": "captured",
                    "rust": flush.summary(),
                    "haskell": None,
                    "mismatches": [],
                }
                for flush in rust_flushes
            ],
            False,
        )

    failed = False
    results: list[dict[str, Any]] = []
    max_len = max(len(rust_flushes), len(haskell_flushes))
    for ordinal in range(max_len):
        rust = rust_flushes[ordinal] if ordinal < len(rust_flushes) else None
        haskell = haskell_flushes[ordinal] if ordinal < len(haskell_flushes) else None
        if rust is None or haskell is None:
            failed = True
            results.append(
                {
                    "ordinal": ordinal,
                    "status": "missing",
                    "rust": rust.summary() if rust is not None else None,
                    "haskell": haskell.summary() if haskell is not None else None,
                    "mismatches": [],
                }
            )
            continue

        mismatches: list[dict[str, str]] = []
        for key in keys:
            rust_value = rust.fields.get(key)
            haskell_value = haskell.fields.get(key)
            if rust_value != haskell_value:
                mismatches.append(
                    {
                        "key": key,
                        "rust": "<missing>" if rust_value is None else rust_value,
                        "haskell": "<missing>" if haskell_value is None else haskell_value,
                    }
                )
        failed = failed or bool(mismatches)
        results.append(
            {
                "ordinal": ordinal,
                "status": "fail" if mismatches else "pass",
                "rust": rust.summary(),
                "haskell": haskell.summary(),
                "mismatches": mismatches,
            }
        )
    return results, failed


def write_summary(path: Path, summary: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(summary, indent=2, sort_keys=True), encoding="utf-8")


def expect_system_exit(func: Any, expected_fragment: str) -> None:
    try:
        func()
    except SystemExit as exc:
        if expected_fragment not in str(exc):
            raise AssertionError(
                f"expected SystemExit containing {expected_fragment!r}, got {exc!r}"
            ) from exc
    else:
        raise AssertionError(f"expected SystemExit containing {expected_fragment!r}")


def run_self_test() -> int:
    line = (
        "steps=200 counts=[Var:12,LamAbs:4,Apply:3,Delay:0,Force:1,Constant:2,"
        "Builtin:1,Constr:0,Case:0] cpu=4600000 mem=20000 before_cpu=4293691 "
        "before_mem=67426 after_cpu=-306309 after_mem=47426 "
        "status=err:out of budget: 200 accumulated steps"
    )
    fields = parse_key_values(line)
    assert fields is not None
    validate_required_keys("self", 1, fields)
    assert fields["steps"] == "200"
    assert fields["status"] == "err:out of budget: 200 accumulated steps"
    counts = parse_counts(fields["counts"], "self")
    assert counts["Var"] == 12
    assert counts["Builtin"] == 1

    rust = Flush("rust", "self", 1, 0, fields, counts)
    same = Flush("haskell", "self", 1, 0, dict(fields), dict(counts))
    results, failed = compare_flushes([rust], [same], DEFAULT_COMPARE_KEYS)
    assert not failed, results
    assert results[0]["status"] == "pass"

    changed = dict(fields)
    changed["cpu"] = "4599999"
    results, failed = compare_flushes(
        [rust],
        [Flush("haskell", "self", 2, 0, changed, dict(counts))],
        DEFAULT_COMPARE_KEYS,
    )
    assert failed
    assert results[0]["mismatches"][0]["key"] == "cpu"

    results, failed = compare_flushes([rust], None, DEFAULT_COMPARE_KEYS)
    assert not failed
    assert results[0]["status"] == "captured"

    missing = dict(fields)
    del missing["status"]
    expect_system_exit(
        lambda: validate_required_keys("self", 2, missing),
        "missing required keys: status",
    )
    expect_system_exit(lambda: parse_counts("Var:12", "self"), "counts must be bracketed")
    expect_system_exit(
        lambda: validate_required_args(
            argparse.Namespace(
                self_test=False,
                rust_log=Path("rust.log"),
                haskell_log=None,
                require_equal=True,
            )
        ),
        "--haskell-log is required",
    )
    _, failed = compare_flushes([rust], [], DEFAULT_COMPARE_KEYS)
    assert failed

    with tempfile.TemporaryDirectory() as tmp:
        artifact = Path(tmp) / "summary.json"
        write_summary(artifact, {"results": results})
        assert artifact.exists()

    print("[ok] compare-gap-bp-cek-flushes self-test passed")
    return 0


def validate_required_args(
    args: argparse.Namespace,
    parser: argparse.ArgumentParser | None = None,
) -> None:
    def fail(message: str) -> None:
        if parser is not None:
            parser.error(message)
        raise SystemExit(message)

    if not args.self_test and args.rust_log is None:
        fail("--rust-log is required unless --self-test is set")
    if not args.self_test and args.require_equal and args.haskell_log is None:
        fail("--haskell-log is required with --require-equal")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Compare Gap BP CEK accumulated-step flush traces"
    )
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--rust-log", type=Path)
    parser.add_argument("--haskell-log", type=Path)
    parser.add_argument(
        "--key",
        action="append",
        help="Flush key to compare; repeatable. Defaults to Gap BP flush parity keys.",
    )
    parser.add_argument(
        "--artifact-dir",
        type=Path,
        default=Path("target/gap-bp-cek-flush-comparison"),
    )
    parser.add_argument(
        "--require-equal",
        action="store_true",
        help="Require --haskell-log and exit non-zero when flush fields differ",
    )
    args = parser.parse_args()
    validate_required_args(args, parser)
    return args


def main() -> int:
    args = parse_args()
    if args.self_test:
        return run_self_test()

    keys = tuple(args.key or DEFAULT_COMPARE_KEYS)
    rust_flushes = load_flushes("rust", args.rust_log)
    haskell_flushes = load_flushes("haskell", args.haskell_log) if args.haskell_log else None
    results, failed = compare_flushes(rust_flushes, haskell_flushes, keys)
    summary = {
        "compare_keys": keys,
        "rust_log": str(args.rust_log),
        "haskell_log": str(args.haskell_log) if args.haskell_log else None,
        "results": results,
    }
    summary_path = args.artifact_dir / "summary.json"
    write_summary(summary_path, summary)
    print(f"wrote {summary_path}")
    for result in results:
        print(f"flush {result['ordinal']}: {result['status']}")
        for mismatch in result["mismatches"][:8]:
            print(
                f"  - {mismatch['key']}: rust={mismatch['rust']} "
                f"haskell={mismatch['haskell']}"
            )
    return 1 if failed and args.require_equal else 0


if __name__ == "__main__":
    sys.exit(main())
