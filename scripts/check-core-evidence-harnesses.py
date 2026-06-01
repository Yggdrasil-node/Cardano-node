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
GAP_BO_SELF_TEST_FIXTURE = (
    ROOT / "target" / "gap-bo-tpraos-vrf-self-test" / "fixture.json"
)
GAP_BP_SELF_TEST_FIXTURE = (
    ROOT / "target" / "gap-bp-traces-self-test" / "fixture.json"
)
R178_SELF_TEST_FIXTURE = (
    ROOT / "target" / "r178-conway-lsq-self-test" / "fixture.json"
)
BLOCKFETCH_SELF_TEST_SUMMARY = (
    ROOT / "target" / "blockfetch-soak-self-test" / "summary.json"
)
EXPECTED_ARTIFACTS = (
    GAP_BO_SELF_TEST_FIXTURE,
    GAP_BP_SELF_TEST_FIXTURE,
    R178_SELF_TEST_FIXTURE,
    BLOCKFETCH_SELF_TEST_SUMMARY,
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


def require_wsl_or_linux() -> None:
    if sys.platform == "win32":
        raise SystemExit(
            "check-core-evidence-harnesses.py must run under WSL/Linux; "
            "use `wsl -e bash -lc \"python3 scripts/check-core-evidence-harnesses.py\"`"
        )


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


def remove_expected_artifacts() -> None:
    for path in EXPECTED_ARTIFACTS:
        if path.is_file() or path.is_symlink():
            path.unlink()


def load_json_object(path: Path, failures: list[str]) -> dict[str, Any]:
    if not path.exists():
        failures.append(f"missing artifact: {path}")
        return {}
    try:
        loaded = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        failures.append(f"failed to parse {path}: {exc}")
        return {}
    if not isinstance(loaded, dict):
        failures.append(f"{path} did not contain a JSON object")
        return {}
    return loaded


def artifact_result(name: str, path: Path, failures: list[str]) -> dict[str, Any]:
    return {
        "name": name,
        "path": str(path),
        "status": "fail" if failures else "pass",
        "failures": failures,
    }


def require_object(
    container: dict[str, Any],
    key: str,
    failures: list[str],
) -> dict[str, Any]:
    value = container.get(key)
    if not isinstance(value, dict):
        failures.append(f"{key} must be an object")
        return {}
    return value


def require_non_empty_list(
    container: dict[str, Any],
    key: str,
    failures: list[str],
) -> list[Any]:
    value = container.get(key)
    if not isinstance(value, list) or not value:
        failures.append(f"{key} must be a non-empty list")
        return []
    return value


def require_generated_at(container: dict[str, Any], failures: list[str]) -> None:
    value = container.get("generated_at_utc")
    if not isinstance(value, str) or not value:
        failures.append("generated_at_utc must be present")
        return
    try:
        dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        failures.append("generated_at_utc must be ISO-8601 parseable")


def require_strict_closeout_mode(
    container: dict[str, Any],
    failures: list[str],
    *,
    require_equal_key: str = "require_equal",
) -> dict[str, Any]:
    mode = require_object(container, "closeout_mode", failures)
    if mode.get("require_haskell") is not True:
        failures.append("closeout_mode.require_haskell must be true")
    if mode.get(require_equal_key) is not True:
        failures.append(f"closeout_mode.{require_equal_key} must be true")
    return mode


def validate_gap_bo_fixture(path: Path) -> dict[str, Any]:
    failures: list[str] = []
    fixture = load_json_object(path, failures)
    if fixture:
        if fixture.get("schema_version") != 1:
            failures.append("schema_version must be 1")
        if fixture.get("blocker") != "gap-bo-tpraos-vrf":
            failures.append("blocker must be gap-bo-tpraos-vrf")
        require_generated_at(fixture, failures)
        require_strict_closeout_mode(fixture, failures)
        if fixture.get("status") != "pass":
            failures.append("status must be pass")
        if fixture.get("target_slot") != 429460:
            failures.append("target_slot must be 429460")
        if fixture.get("mismatches") != []:
            failures.append("mismatches must be empty")

        required_keys = require_non_empty_list(fixture, "required_keys", failures)
        compare_keys = require_non_empty_list(fixture, "compare_keys", failures)
        rust_fields = require_object(fixture, "rust_fields", failures)
        haskell_fields = require_object(fixture, "haskell_fields", failures)

        for key in required_keys:
            if key not in rust_fields:
                failures.append(f"rust_fields.{key} must be present")
            if key not in haskell_fields:
                failures.append(f"haskell_fields.{key} must be present")
        if str(fixture.get("target_slot")) != rust_fields.get("slot"):
            failures.append("rust_fields.slot must match target_slot")
        if str(fixture.get("target_slot")) != haskell_fields.get("slot"):
            failures.append("haskell_fields.slot must match target_slot")
        for key in compare_keys:
            if rust_fields.get(key) != haskell_fields.get(key):
                failures.append(f"compared field {key} differs in self-test fixture")

    return artifact_result("gap-bo-fixture", path, failures)


def validate_gap_bp_fixture(path: Path) -> dict[str, Any]:
    failures: list[str] = []
    fixture = load_json_object(path, failures)
    if fixture:
        if fixture.get("schema_version") != 1:
            failures.append("schema_version must be 1")
        if fixture.get("blocker") != "gap-bp-plutus-v2-traces":
            failures.append("blocker must be gap-bp-plutus-v2-traces")
        require_generated_at(fixture, failures)
        require_strict_closeout_mode(fixture, failures)
        if fixture.get("status") != "pass":
            failures.append("status must be pass")
        expected_trace_id = fixture.get("expected_trace_id")
        if not expected_trace_id:
            failures.append("expected_trace_id must be present")

        trace_identity = require_object(fixture, "trace_identity", failures)
        if trace_identity.get("violations") != []:
            failures.append("trace_identity.violations must be empty")
        require_object(trace_identity, "observed", failures)

        script_context = require_object(fixture, "script_context", failures)
        script_comparison = require_object(script_context, "comparison", failures)
        if script_comparison.get("byte_equal") is not True:
            failures.append("script_context.comparison.byte_equal must be true")

        for child_key in ("cek_flushes", "builtin_costs"):
            child = require_object(fixture, child_key, failures)
            results = require_non_empty_list(child, "results", failures)
            for index, result in enumerate(results):
                if not isinstance(result, dict):
                    failures.append(f"{child_key}.results[{index}] must be an object")
                    continue
                if result.get("status") != "pass":
                    failures.append(f"{child_key}.results[{index}].status must be pass")
                for side in ("rust", "haskell"):
                    entry = result.get(side)
                    if not isinstance(entry, dict):
                        failures.append(
                            f"{child_key}.results[{index}].{side} must be an object"
                        )
                        continue
                    trace_id = (entry.get("fields") or {}).get("trace_id")
                    if expected_trace_id and trace_id != expected_trace_id:
                        failures.append(
                            f"{child_key}.results[{index}].{side}.trace_id "
                            "must match expected_trace_id"
                        )

    return artifact_result("gap-bp-fixture", path, failures)


def validate_r178_fixture(path: Path) -> dict[str, Any]:
    failures: list[str] = []
    fixture = load_json_object(path, failures)
    if fixture:
        if fixture.get("schema_version") != 1:
            failures.append("schema_version must be 1")
        if fixture.get("blocker") != "r178-conway-lsq":
            failures.append("blocker must be r178-conway-lsq")
        require_generated_at(fixture, failures)
        mode = require_object(fixture, "closeout_mode", failures)
        if mode.get("require_haskell") is not True:
            failures.append("closeout_mode.require_haskell must be true")
        if not (
            mode.get("require_byte_equal") is True
            or mode.get("require_normalized_equal") is True
        ):
            failures.append("closeout_mode must require byte or normalized equality")
        if mode.get("require_byte_equal") is not fixture.get("require_byte_equal"):
            failures.append("closeout_mode.require_byte_equal must match fixture")
        if mode.get("require_normalized_equal") is not fixture.get(
            "require_normalized_equal"
        ):
            failures.append(
                "closeout_mode.require_normalized_equal must match fixture"
            )
        if fixture.get("status") != "pass":
            failures.append("status must be pass")
        if not (
            fixture.get("require_byte_equal")
            or fixture.get("require_normalized_equal")
        ):
            failures.append("fixture must require byte or normalized equality")
        if fixture.get("require_normalized_equal") is not True:
            failures.append("self-test fixture must require normalized equality")
        cardano_cli_version = require_object(
            fixture,
            "cardano_cli_version",
            failures,
        )
        if "command" in cardano_cli_version:
            failures.append("cardano_cli_version must not include command")
        if not cardano_cli_version.get("stdout_sha256"):
            failures.append("cardano_cli_version.stdout_sha256 must be present")

        queries = require_object(fixture, "queries", failures)
        expected_queries = {"gov-state", "constitution", "committee-state"}
        if set(queries) != expected_queries:
            failures.append(
                "queries must contain gov-state, constitution, and committee-state"
            )
        for query, result in queries.items():
            if not isinstance(result, dict):
                failures.append(f"queries.{query} must be an object")
                continue
            if result.get("status") != "pass":
                failures.append(f"queries.{query}.status must be pass")
            normalized_json = result.get("normalized_json")
            if not normalized_json:
                failures.append(f"queries.{query}.normalized_json must be present")
            raw_stdout = require_object(
                result,
                "raw_stdout_comparison",
                failures,
            )
            if "byte_equal" not in raw_stdout:
                failures.append(
                    f"queries.{query}.raw_stdout_comparison.byte_equal must be present"
                )
            for side in ("yggdrasil", "haskell"):
                side_result = require_object(result, side, failures)
                if "command" in side_result:
                    failures.append(f"queries.{query}.{side} must not include command")
                if side_result.get("timed_out"):
                    failures.append(f"queries.{query}.{side}.timed_out must be false")
                if side_result.get("normalized_json") != normalized_json:
                    failures.append(
                        f"queries.{query}.{side}.normalized_json must match query"
                    )

    return artifact_result("r178-fixture", path, failures)


def validate_blockfetch_self_test_summary(path: Path) -> dict[str, Any]:
    failures: list[str] = []
    summary = load_json_object(path, failures)

    if summary:
        if summary.get("schema_version") != 1:
            failures.append("schema_version must be 1")
        if summary.get("blocker") != "blockfetch-section-6.5":
            failures.append("blocker must be blockfetch-section-6.5")
        require_generated_at(summary, failures)
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

    return artifact_result("blockfetch-soak-summary", path, failures)


def validate_artifacts() -> list[dict[str, Any]]:
    return [
        validate_gap_bo_fixture(GAP_BO_SELF_TEST_FIXTURE),
        validate_gap_bp_fixture(GAP_BP_SELF_TEST_FIXTURE),
        validate_r178_fixture(R178_SELF_TEST_FIXTURE),
        validate_blockfetch_self_test_summary(BLOCKFETCH_SELF_TEST_SUMMARY),
    ]


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
    require_wsl_or_linux()
    remove_expected_artifacts()
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
