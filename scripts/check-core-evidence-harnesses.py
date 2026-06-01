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
BLOCKFETCH_SELF_TEST_SUMMARY = (
    ROOT / "target" / "blockfetch-soak-self-test" / "summary.json"
)


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


def numeric(value: Any, default: float = 0) -> float:
    if value is None:
        return default
    if isinstance(value, bool):
        return 1.0 if value else 0.0
    if isinstance(value, (int, float)):
        return float(value)
    try:
        return float(str(value))
    except ValueError:
        return default


def validate_blockfetch_self_test_summary(path: Path) -> dict[str, Any]:
    failures: list[str] = []
    summary: dict[str, Any] = {}
    if not path.exists():
        failures.append(f"missing artifact: {path}")
    else:
        try:
            loaded = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            failures.append(f"failed to parse {path}: {exc}")
        else:
            if not isinstance(loaded, dict):
                failures.append(f"{path} did not contain a JSON object")
            else:
                summary = loaded

    if summary:
        if summary.get("schema_version") != 1:
            failures.append("schema_version must be 1")
        if summary.get("blocker") != "blockfetch-section-6.5":
            failures.append("blocker must be blockfetch-section-6.5")
        if summary.get("status") != "pass":
            failures.append("status must be pass")

        worker = summary.get("worker_assertions")
        progress = summary.get("progress_assertions")
        tip = summary.get("tip_comparison")
        run = summary.get("run")
        artifacts = summary.get("artifacts")
        if not isinstance(worker, dict):
            failures.append("worker_assertions must be an object")
            worker = {}
        if not isinstance(progress, dict):
            failures.append("progress_assertions must be an object")
            progress = {}
        if not isinstance(tip, dict):
            failures.append("tip_comparison must be an object")
            tip = {}
        if not isinstance(run, dict):
            failures.append("run must be an object")
            run = {}
        if not isinstance(artifacts, dict):
            failures.append("artifacts must be an object")
            artifacts = {}

        expected_workers = numeric(worker.get("expected_workers"))
        if expected_workers < 1:
            failures.append("expected_workers must be at least 1")
        if not worker.get("require_workers"):
            failures.append("require_workers must be true")
        if numeric(worker.get("workers_registered_max")) < expected_workers:
            failures.append("workers_registered_max must reach expected_workers")
        if numeric(worker.get("workers_registered_final")) < expected_workers:
            failures.append("workers_registered_final must reach expected_workers")
        if numeric(worker.get("workers_migrated_total")) < expected_workers:
            failures.append("workers_migrated_total must reach expected_workers")
        if numeric(worker.get("worker_shortfall_samples")) != 0:
            failures.append("worker_shortfall_samples must be 0")

        if not progress.get("require_progress"):
            failures.append("require_progress must be true")
        blocks = progress.get("blocks_synced")
        slot = progress.get("current_slot")
        if not isinstance(blocks, dict):
            failures.append("blocks_synced must be an object")
            blocks = {}
        if not isinstance(slot, dict):
            failures.append("current_slot must be an object")
            slot = {}
        if not (
            numeric(blocks.get("end")) > numeric(blocks.get("start"))
            or numeric(slot.get("end")) > numeric(slot.get("start"))
        ):
            failures.append("blocks_synced or current_slot must increase")

        if not tip.get("require_tip_comparison"):
            failures.append("require_tip_comparison must be true")
        if numeric(tip.get("tip_compare_passes")) < numeric(
            tip.get("min_tip_compare_passes")
        ):
            failures.append("tip_compare_passes must reach min_tip_compare_passes")
        if numeric(tip.get("min_tip_compare_passes")) < 2:
            failures.append("min_tip_compare_passes must be at least 2")

        if numeric(run.get("tip_query_timeout_seconds")) >= numeric(
            run.get("compare_interval_seconds")
        ):
            failures.append(
                "tip_query_timeout_seconds must be below compare_interval_seconds"
            )

        for key in ("run_dir", "log_dir", "metrics_dir", "node_log", "summary_txt"):
            if not artifacts.get(key):
                failures.append(f"artifacts.{key} must be present")

    return {
        "name": "blockfetch-soak-summary",
        "path": str(path),
        "status": "fail" if failures else "pass",
        "failures": failures,
    }


def validate_artifacts() -> list[dict[str, Any]]:
    return [validate_blockfetch_self_test_summary(BLOCKFETCH_SELF_TEST_SUMMARY)]


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
    artifact_checks = validate_artifacts()
    failed = [result for result in results if result["status"] != "pass"]
    failed_artifacts = [
        result for result in artifact_checks if result["status"] != "pass"
    ]

    summary = {
        "generated_at_utc": dt.datetime.now(dt.UTC).isoformat(),
        "status": "fail" if failed or failed_artifacts else "pass",
        "artifact_checks": artifact_checks,
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
    for check in artifact_checks:
        print(f"[{check['status']}] artifact {check['name']}")
        for failure in check["failures"]:
            print(f"  - {failure}")
    print(f"wrote {summary_path}")
    return 1 if failed or failed_artifacts else 0


if __name__ == "__main__":
    sys.exit(main())
