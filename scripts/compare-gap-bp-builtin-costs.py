#!/usr/bin/env python3
"""Compare Gap BP per-builtin Plutus cost traces.

Rust emits one line per evaluated builtin when `YGG_DUMP_BUILTIN_COSTS=1`.
A Haskell reference capture can be transformed into the same key-value shape,
then this helper compares builtin name, argument memory sizes, charged
CPU/memory, and remaining budget by ordinal index.
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
    "trace_id",
    "fun",
    "args",
    "cpu",
    "mem",
    "remaining_cpu",
    "remaining_mem",
)
DEFAULT_COMPARE_KEYS = REQUIRED_KEYS


@dataclass(frozen=True)
class BuiltinCost:
    label: str
    source: str
    line_number: int
    ordinal: int
    fields: dict[str, str]
    arg_sizes: list[int]

    def summary(self) -> dict[str, Any]:
        return {
            "label": self.label,
            "source": self.source,
            "line_number": self.line_number,
            "ordinal": self.ordinal,
            "fields": self.fields,
            "arg_sizes": self.arg_sizes,
        }


def parse_key_values(line: str) -> dict[str, str] | None:
    if "fun=" not in line or "args=[" not in line:
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


def parse_arg_sizes(value: str, label: str) -> list[int]:
    if not value.startswith("[") or not value.endswith("]"):
        raise SystemExit(f"{label}: args must be bracketed")
    body = value[1:-1].strip()
    if not body:
        return []

    sizes: list[int] = []
    for raw_size in body.split(","):
        raw_size = raw_size.strip()
        try:
            sizes.append(int(raw_size))
        except ValueError as exc:
            raise SystemExit(f"{label}: invalid arg size {raw_size!r}") from exc
    return sizes


def validate_required_keys(label: str, line_number: int, fields: dict[str, str]) -> None:
    missing = [key for key in REQUIRED_KEYS if key not in fields]
    if missing:
        joined = ", ".join(missing)
        raise SystemExit(f"{label}: line {line_number}: missing required keys: {joined}")


def load_builtin_costs(label: str, path: Path) -> list[BuiltinCost]:
    costs: list[BuiltinCost] = []
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        fields = parse_key_values(line)
        if fields is None:
            continue
        validate_required_keys(str(path), line_number, fields)
        arg_sizes = parse_arg_sizes(fields["args"], f"{label}:{line_number}")
        costs.append(
            BuiltinCost(
                label=label,
                source=str(path),
                line_number=line_number,
                ordinal=len(costs),
                fields=fields,
                arg_sizes=arg_sizes,
            )
        )
    if not costs:
        raise SystemExit(f"{label}: no builtin cost lines found in {path}")
    return costs


def compare_builtin_costs(
    rust_costs: list[BuiltinCost],
    haskell_costs: list[BuiltinCost] | None,
    keys: tuple[str, ...],
) -> tuple[list[dict[str, Any]], bool]:
    if haskell_costs is None:
        return (
            [
                {
                    "ordinal": cost.ordinal,
                    "status": "captured",
                    "rust": cost.summary(),
                    "haskell": None,
                    "mismatches": [],
                }
                for cost in rust_costs
            ],
            False,
        )

    failed = False
    results: list[dict[str, Any]] = []
    max_len = max(len(rust_costs), len(haskell_costs))
    for ordinal in range(max_len):
        rust = rust_costs[ordinal] if ordinal < len(rust_costs) else None
        haskell = haskell_costs[ordinal] if ordinal < len(haskell_costs) else None
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
        "trace_id=aa:bb:V2 fun=AppendByteString args=[12,8] cpu=100001 mem=64 "
        "remaining_cpu=4293691 remaining_mem=67426"
    )
    fields = parse_key_values(line)
    assert fields is not None
    validate_required_keys("self", 1, fields)
    assert fields["trace_id"] == "aa:bb:V2"
    assert fields["fun"] == "AppendByteString"
    assert parse_arg_sizes(fields["args"], "self") == [12, 8]
    assert parse_arg_sizes("[]", "self") == []

    rust = BuiltinCost("rust", "self", 1, 0, fields, [12, 8])
    same = BuiltinCost("haskell", "self", 1, 0, dict(fields), [12, 8])
    results, failed = compare_builtin_costs([rust], [same], DEFAULT_COMPARE_KEYS)
    assert not failed, results
    assert results[0]["status"] == "pass"

    changed = dict(fields)
    changed["remaining_cpu"] = "4293690"
    results, failed = compare_builtin_costs(
        [rust],
        [BuiltinCost("haskell", "self", 2, 0, changed, [12, 8])],
        DEFAULT_COMPARE_KEYS,
    )
    assert failed
    assert results[0]["mismatches"][0]["key"] == "remaining_cpu"

    results, failed = compare_builtin_costs([rust], None, DEFAULT_COMPARE_KEYS)
    assert not failed
    assert results[0]["status"] == "captured"

    missing = dict(fields)
    del missing["mem"]
    expect_system_exit(
        lambda: validate_required_keys("self", 2, missing),
        "missing required keys: mem",
    )
    expect_system_exit(lambda: parse_arg_sizes("12,8", "self"), "args must be bracketed")
    expect_system_exit(lambda: parse_arg_sizes("[12,x]", "self"), "invalid arg size")
    expect_system_exit(
        lambda: validate_required_args(
            argparse.Namespace(
                self_test=False,
                rust_log=Path("rust.log"),
                haskell_log=None,
                require_haskell=True,
                require_equal=True,
            )
        ),
        "--haskell-log is required with --require-haskell",
    )
    expect_system_exit(
        lambda: validate_required_args(
            argparse.Namespace(
                self_test=False,
                rust_log=Path("rust.log"),
                haskell_log=Path("haskell.log"),
                require_haskell=False,
                require_equal=True,
            )
        ),
        "--require-equal requires --require-haskell",
    )
    expect_system_exit(
        lambda: validate_required_args(
            argparse.Namespace(
                self_test=False,
                rust_log=Path("rust.log"),
                haskell_log=Path("haskell.log"),
                require_haskell=True,
                require_equal=False,
            )
        ),
        "--require-haskell requires --require-equal",
    )
    _, failed = compare_builtin_costs([rust], [], DEFAULT_COMPARE_KEYS)
    assert failed

    with tempfile.TemporaryDirectory() as tmp:
        rust_path = Path(tmp) / "rust.log"
        rust_path.write_text(f"noise\n{line}\n", encoding="utf-8")
        loaded = load_builtin_costs("rust", rust_path)
        assert loaded[0].arg_sizes == [12, 8]

        artifact = Path(tmp) / "summary.json"
        write_summary(artifact, {"results": results})
        assert artifact.exists()

    print("[ok] compare-gap-bp-builtin-costs self-test passed")
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
    if not args.self_test and args.require_haskell and not args.require_equal:
        fail("--require-haskell requires --require-equal")
    if not args.self_test and args.require_equal and not args.require_haskell:
        fail("--require-equal requires --require-haskell")
    if not args.self_test and args.require_haskell and args.haskell_log is None:
        fail("--haskell-log is required with --require-haskell")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Compare Gap BP per-builtin Plutus cost traces"
    )
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--rust-log", type=Path)
    parser.add_argument("--haskell-log", type=Path)
    parser.add_argument(
        "--key",
        action="append",
        help="Builtin-cost key to compare; repeatable. Defaults to Gap BP parity keys.",
    )
    parser.add_argument(
        "--artifact-dir",
        type=Path,
        default=Path("target/gap-bp-builtin-cost-comparison"),
    )
    parser.add_argument(
        "--require-equal",
        action="store_true",
        help="Exit non-zero when required Haskell builtin costs differ",
    )
    parser.add_argument(
        "--require-haskell",
        action="store_true",
        help="Require --haskell-log for Gap BP builtin-cost closeout mode",
    )
    args = parser.parse_args()
    validate_required_args(args, parser)
    return args


def main() -> int:
    args = parse_args()
    if args.self_test:
        return run_self_test()

    keys = tuple(args.key or DEFAULT_COMPARE_KEYS)
    rust_costs = load_builtin_costs("rust", args.rust_log)
    haskell_costs = (
        load_builtin_costs("haskell", args.haskell_log) if args.haskell_log else None
    )
    results, failed = compare_builtin_costs(rust_costs, haskell_costs, keys)
    summary = {
        "compare_keys": keys,
        "rust_log": str(args.rust_log),
        "haskell_log": str(args.haskell_log) if args.haskell_log else None,
        "require_haskell": args.require_haskell,
        "require_equal": args.require_equal,
        "results": results,
    }
    summary_path = args.artifact_dir / "summary.json"
    write_summary(summary_path, summary)
    print(f"wrote {summary_path}")
    for result in results:
        print(f"builtin {result['ordinal']}: {result['status']}")
        for mismatch in result["mismatches"][:8]:
            print(
                f"  - {mismatch['key']}: rust={mismatch['rust']} "
                f"haskell={mismatch['haskell']}"
            )
    return 1 if failed and args.require_equal else 0


if __name__ == "__main__":
    sys.exit(main())
