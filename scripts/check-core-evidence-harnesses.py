#!/usr/bin/env python3
"""Run local self-tests for core parity evidence harnesses.

This is a fast preflight for the operator evidence helpers used by the open
Gap BO, Gap BP, R178, and BlockFetch closure arcs. It does not prove live
Haskell parity; it proves the local comparison harnesses still parse, compare,
and report evidence before an operator starts the longer socket/soak runs.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_ARTIFACT_DIR = ROOT / "target" / "core-evidence-harnesses"


@dataclass(frozen=True)
class Case:
    name: str
    command: list[str]


def run_case(case: Case) -> dict[str, Any]:
    started = time.monotonic()
    try:
        proc = subprocess.run(
            case.command,
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=False,
        )
    except FileNotFoundError as exc:
        duration = time.monotonic() - started
        return {
            "name": case.name,
            "command": case.command,
            "status": "fail",
            "exit_code": None,
            "duration_secs": round(duration, 3),
            "stdout": "",
            "stderr": str(exc),
        }

    duration = time.monotonic() - started
    return {
        "name": case.name,
        "command": case.command,
        "status": "pass" if proc.returncode == 0 else "fail",
        "exit_code": proc.returncode,
        "duration_secs": round(duration, 3),
        "stdout": proc.stdout,
        "stderr": proc.stderr,
    }


def tail(text: str, line_count: int = 12) -> str:
    lines = text.splitlines()
    if len(lines) <= line_count:
        return text.strip()
    return "\n".join(lines[-line_count:])


def build_cases() -> list[Case]:
    python = sys.executable
    cases = [
        Case(
            "gap-bo-tpraos-vrf",
            [python, "scripts/compare-gap-bo-tpraos-vrf.py", "--self-test"],
        ),
        Case(
            "gap-bp-script-context",
            [python, "scripts/compare-gap-bp-script-context.py", "--self-test"],
        ),
        Case(
            "gap-bp-cek-flushes",
            [python, "scripts/compare-gap-bp-cek-flushes.py", "--self-test"],
        ),
        Case(
            "gap-bp-builtin-costs",
            [python, "scripts/compare-gap-bp-builtin-costs.py", "--self-test"],
        ),
        Case(
            "gap-bp-traces",
            [python, "scripts/compare-gap-bp-traces.py", "--self-test"],
        ),
        Case(
            "r178-conway-lsq",
            [python, "scripts/compare-conway-lsq.py", "--self-test"],
        ),
    ]

    bash = shutil.which("bash")
    if bash is None:
        cases.append(
            Case(
                "tip-comparison",
                ["bash", "scripts/compare_tip_to_haskell.sh", "--self-test"],
            )
        )
        cases.append(
            Case(
                "blockfetch-soak",
                [
                    "bash",
                    "scripts/parallel_blockfetch_soak.sh",
                    "--self-test",
                ],
            )
        )
    else:
        cases.append(
            Case(
                "tip-comparison",
                [bash, "scripts/compare_tip_to_haskell.sh", "--self-test"],
            )
        )
        cases.append(
            Case(
                "blockfetch-soak",
                [bash, "scripts/parallel_blockfetch_soak.sh", "--self-test"],
            )
        )
    return cases


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run local self-tests for core parity evidence harnesses"
    )
    parser.add_argument(
        "--artifact-dir",
        type=Path,
        default=DEFAULT_ARTIFACT_DIR,
        help="Directory for summary.json",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    results = [run_case(case) for case in build_cases()]
    failed = [result for result in results if result["status"] != "pass"]

    summary = {
        "generated_at_utc": dt.datetime.now(dt.UTC).isoformat(),
        "status": "fail" if failed else "pass",
        "results": results,
    }
    args.artifact_dir.mkdir(parents=True, exist_ok=True)
    summary_path = args.artifact_dir / "summary.json"
    summary_path.write_text(
        json.dumps(summary, indent=2, sort_keys=True),
        encoding="utf-8",
    )

    for result in results:
        print(
            f"[{result['status']}] {result['name']} "
            f"({result['duration_secs']:.3f}s)"
        )
        if result["status"] != "pass":
            if result["stdout"]:
                print("stdout:")
                print(tail(result["stdout"]))
            if result["stderr"]:
                print("stderr:")
                print(tail(result["stderr"]))
    print(f"wrote {summary_path}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
