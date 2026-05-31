#!/usr/bin/env python3
"""Run the complete Gap BP Plutus evidence comparison set.

This aggregate harness composes the three focused Gap BP comparators:

- ScriptContext CBOR bytes
- accumulated CEK step-budget flushes
- per-builtin budget charges

Use capture mode with only Rust logs while preparing artifacts. Use
`--require-haskell --require-equal` for the parity closeout run.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_ARTIFACT_DIR = ROOT / "target" / "gap-bp-trace-comparison"


@dataclass(frozen=True)
class Case:
    name: str
    command: list[str]
    summary_path: Path


def run_case(case: Case) -> dict[str, Any]:
    started = time.monotonic()
    proc = subprocess.run(
        case.command,
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    duration = time.monotonic() - started
    child_summary: dict[str, Any] | None = None
    if case.summary_path.exists():
        child_summary = json.loads(case.summary_path.read_text(encoding="utf-8"))
    return {
        "name": case.name,
        "command": case.command,
        "status": "pass" if proc.returncode == 0 else "fail",
        "exit_code": proc.returncode,
        "duration_secs": round(duration, 3),
        "summary_path": str(case.summary_path),
        "child_summary": child_summary,
        "stdout": proc.stdout,
        "stderr": proc.stderr,
    }


def require_path(label: str, path: Path | None) -> Path:
    if path is None:
        raise SystemExit(f"{label} is required")
    if not path.exists():
        raise SystemExit(f"{label} does not exist: {path}")
    return path


def maybe_path(label: str, path: Path | None, require: bool) -> Path | None:
    if path is None:
        if require:
            raise SystemExit(f"{label} is required with --require-haskell")
        return None
    if not path.exists():
        raise SystemExit(f"{label} does not exist: {path}")
    return path


def build_cases(args: argparse.Namespace) -> list[Case]:
    rust_script_context = require_path(
        "--rust-script-context", args.rust_script_context
    )
    rust_cek_flushes = require_path("--rust-cek-flushes", args.rust_cek_flushes)
    rust_builtin_costs = require_path("--rust-builtin-costs", args.rust_builtin_costs)

    haskell_script_context = maybe_path(
        "--haskell-script-context",
        args.haskell_script_context,
        args.require_haskell,
    )
    haskell_cek_flushes = maybe_path(
        "--haskell-cek-flushes",
        args.haskell_cek_flushes,
        args.require_haskell,
    )
    haskell_builtin_costs = maybe_path(
        "--haskell-builtin-costs",
        args.haskell_builtin_costs,
        args.require_haskell,
    )

    python = sys.executable
    script_context_dir = args.artifact_dir / "script-context"
    cek_flushes_dir = args.artifact_dir / "cek-flushes"
    builtin_costs_dir = args.artifact_dir / "builtin-costs"

    script_context_cmd = [
        python,
        "scripts/compare-gap-bp-script-context.py",
        "--rust-log",
        str(rust_script_context),
        "--artifact-dir",
        str(script_context_dir),
    ]
    if haskell_script_context is not None:
        script_context_cmd.extend(["--haskell-log", str(haskell_script_context)])
    if args.require_equal:
        script_context_cmd.append("--require-byte-equal")

    cek_flushes_cmd = [
        python,
        "scripts/compare-gap-bp-cek-flushes.py",
        "--rust-log",
        str(rust_cek_flushes),
        "--artifact-dir",
        str(cek_flushes_dir),
    ]
    if haskell_cek_flushes is not None:
        cek_flushes_cmd.extend(["--haskell-log", str(haskell_cek_flushes)])
    if args.require_equal:
        cek_flushes_cmd.append("--require-equal")

    builtin_costs_cmd = [
        python,
        "scripts/compare-gap-bp-builtin-costs.py",
        "--rust-log",
        str(rust_builtin_costs),
        "--artifact-dir",
        str(builtin_costs_dir),
    ]
    if haskell_builtin_costs is not None:
        builtin_costs_cmd.extend(["--haskell-log", str(haskell_builtin_costs)])
    if args.require_equal:
        builtin_costs_cmd.append("--require-equal")

    return [
        Case(
            "script-context",
            script_context_cmd,
            script_context_dir / "summary.json",
        ),
        Case("cek-flushes", cek_flushes_cmd, cek_flushes_dir / "summary.json"),
        Case("builtin-costs", builtin_costs_cmd, builtin_costs_dir / "summary.json"),
    ]


def validate_required_args(
    args: argparse.Namespace,
    parser: argparse.ArgumentParser | None = None,
) -> None:
    def fail(message: str) -> None:
        if parser is not None:
            parser.error(message)
        raise SystemExit(message)

    if args.self_test:
        return
    for label, value in (
        ("--rust-script-context", args.rust_script_context),
        ("--rust-cek-flushes", args.rust_cek_flushes),
        ("--rust-builtin-costs", args.rust_builtin_costs),
    ):
        if value is None:
            fail(f"{label} is required unless --self-test is set")
    if args.require_equal:
        missing = [
            label
            for label, value in (
                ("--haskell-script-context", args.haskell_script_context),
                ("--haskell-cek-flushes", args.haskell_cek_flushes),
                ("--haskell-builtin-costs", args.haskell_builtin_costs),
            )
            if value is None
        ]
        if missing:
            fail("--require-equal requires " + ", ".join(missing))


def write_summary(path: Path, summary: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(summary, indent=2, sort_keys=True), encoding="utf-8")


def run_comparison(args: argparse.Namespace) -> tuple[dict[str, Any], bool]:
    args.artifact_dir.mkdir(parents=True, exist_ok=True)
    results = [run_case(case) for case in build_cases(args)]
    failed = [result for result in results if result["status"] != "pass"]
    identity = validate_trace_identity(results, require_haskell=args.require_haskell)
    summary = {
        "generated_at_utc": dt.datetime.now(dt.UTC).isoformat(),
        "status": "fail" if failed or identity["violations"] else "pass",
        "require_haskell": args.require_haskell,
        "require_equal": args.require_equal,
        "trace_identity": identity,
        "results": results,
    }
    write_summary(args.artifact_dir / "summary.json", summary)
    return summary, bool(failed or identity["violations"])


def script_context_trace_id(summary: dict[str, Any], side: str) -> str | None:
    entry = summary.get(side)
    if entry is None:
        return None
    metadata = entry.get("metadata") or {}
    trace_id = metadata.get("trace_id")
    if trace_id:
        return trace_id
    tx_hash = metadata.get("tx_hash")
    script_hash = metadata.get("script_hash")
    version = metadata.get("version")
    if tx_hash and script_hash and version:
        return f"{tx_hash}:{script_hash}:{version}"
    return None


def trace_ids_from_results(summary: dict[str, Any], side: str) -> list[str]:
    trace_ids: set[str] = set()
    for result in summary.get("results", []):
        entry = result.get(side)
        if entry is None:
            continue
        trace_id = (entry.get("fields") or {}).get("trace_id")
        if trace_id:
            trace_ids.add(trace_id)
    return sorted(trace_ids)


def validate_trace_identity(
    results: list[dict[str, Any]],
    require_haskell: bool,
) -> dict[str, Any]:
    by_name = {result["name"]: result.get("child_summary") for result in results}
    violations: list[str] = []
    observed: dict[str, dict[str, Any]] = {}

    script_summary = by_name.get("script-context")
    cek_summary = by_name.get("cek-flushes")
    builtin_summary = by_name.get("builtin-costs")
    if not isinstance(script_summary, dict):
        violations.append("missing script-context child summary")
        script_summary = {}
    if not isinstance(cek_summary, dict):
        violations.append("missing cek-flushes child summary")
        cek_summary = {}
    if not isinstance(builtin_summary, dict):
        violations.append("missing builtin-costs child summary")
        builtin_summary = {}

    for side in ("rust", "haskell"):
        if side == "haskell" and not require_haskell:
            continue
        script_trace_id = script_context_trace_id(script_summary, side)
        cek_trace_ids = trace_ids_from_results(cek_summary, side)
        builtin_trace_ids = trace_ids_from_results(builtin_summary, side)
        observed[side] = {
            "script_context": script_trace_id,
            "cek_flushes": cek_trace_ids,
            "builtin_costs": builtin_trace_ids,
        }

        if script_trace_id is None:
            violations.append(f"{side}: script-context evidence has no trace identity")
            continue
        if cek_trace_ids != [script_trace_id]:
            violations.append(
                f"{side}: CEK flush trace_id set {cek_trace_ids} does not match {script_trace_id}"
            )
        if builtin_trace_ids != [script_trace_id]:
            violations.append(
                f"{side}: builtin-cost trace_id set {builtin_trace_ids} does not match {script_trace_id}"
            )

    if require_haskell and not violations:
        rust_id = observed.get("rust", {}).get("script_context")
        haskell_id = observed.get("haskell", {}).get("script_context")
        if rust_id != haskell_id:
            violations.append(f"rust/haskell trace_id mismatch: {rust_id} != {haskell_id}")

    return {
        "observed": observed,
        "violations": violations,
    }


def run_self_test() -> int:
    script_context = (
        "YGG_DUMP_SCRIPT_CONTEXT: trace_id=aa:bb:V2 tx_hash=aa script_hash=bb "
        "version=V2 cbor_len=4 cbor_hex=d8799fff\n"
    )
    cek_flush = (
        "trace_id=aa:bb:V2 steps=4 counts=[Constant:1,Var:1,LamAbs:1,Apply:1,Delay:0,Force:0,"
        "Builtin:0,Constr:0,Case:0] cpu=100 mem=10 before_cpu=1000 "
        "before_mem=500 after_cpu=900 after_mem=490 status=ok\n"
    )
    builtin_cost = (
        "trace_id=aa:bb:V2 fun=AddInteger args=[1,1] cpu=1000 mem=1000 "
        "remaining_cpu=5000 remaining_mem=4000\n"
    )

    with tempfile.TemporaryDirectory(prefix="gap-bp-traces-self-") as tmp:
        root = Path(tmp)
        rust_script = root / "rust-script-context.log"
        rust_cek = root / "rust-cek-flushes.log"
        rust_builtin = root / "rust-builtin-costs.log"
        haskell_script = root / "haskell-script-context.log"
        haskell_cek = root / "haskell-cek-flushes.log"
        haskell_builtin = root / "haskell-builtin-costs.log"
        rust_script.write_text(script_context, encoding="utf-8")
        rust_cek.write_text(cek_flush, encoding="utf-8")
        rust_builtin.write_text(builtin_cost, encoding="utf-8")
        haskell_script.write_text(script_context, encoding="utf-8")
        haskell_cek.write_text(cek_flush, encoding="utf-8")
        haskell_builtin.write_text(builtin_cost, encoding="utf-8")

        capture_args = argparse.Namespace(
            rust_script_context=rust_script,
            haskell_script_context=None,
            rust_cek_flushes=rust_cek,
            haskell_cek_flushes=None,
            rust_builtin_costs=rust_builtin,
            haskell_builtin_costs=None,
            artifact_dir=root / "capture",
            require_haskell=False,
            require_equal=False,
        )
        summary, failed = run_comparison(capture_args)
        assert not failed, summary
        assert summary["status"] == "pass"
        assert summary["results"][0]["child_summary"]["comparison"] is None
        assert summary["trace_identity"]["observed"]["rust"]["script_context"] == "aa:bb:V2"

        equal_args = argparse.Namespace(
            rust_script_context=rust_script,
            haskell_script_context=haskell_script,
            rust_cek_flushes=rust_cek,
            haskell_cek_flushes=haskell_cek,
            rust_builtin_costs=rust_builtin,
            haskell_builtin_costs=haskell_builtin,
            artifact_dir=root / "equal",
            require_haskell=True,
            require_equal=True,
        )
        summary, failed = run_comparison(equal_args)
        assert not failed, summary
        assert summary["status"] == "pass"
        assert summary["results"][0]["child_summary"]["comparison"]["byte_equal"] is True
        assert not summary["trace_identity"]["violations"]

        haskell_builtin.write_text(
            builtin_cost.replace("remaining_cpu=5000", "remaining_cpu=4999"),
            encoding="utf-8",
        )
        mismatch_args = argparse.Namespace(
            rust_script_context=rust_script,
            haskell_script_context=haskell_script,
            rust_cek_flushes=rust_cek,
            haskell_cek_flushes=haskell_cek,
            rust_builtin_costs=rust_builtin,
            haskell_builtin_costs=haskell_builtin,
            artifact_dir=root / "mismatch",
            require_haskell=True,
            require_equal=True,
        )
        summary, failed = run_comparison(mismatch_args)
        assert failed, summary
        assert summary["status"] == "fail"
        assert summary["results"][2]["name"] == "builtin-costs"
        assert summary["results"][2]["status"] == "fail"

        haskell_builtin.write_text(builtin_cost, encoding="utf-8")
        rust_cek.write_text(cek_flush.replace("aa:bb:V2", "aa:cc:V2"), encoding="utf-8")
        trace_mismatch_args = argparse.Namespace(
            rust_script_context=rust_script,
            haskell_script_context=haskell_script,
            rust_cek_flushes=rust_cek,
            haskell_cek_flushes=haskell_cek,
            rust_builtin_costs=rust_builtin,
            haskell_builtin_costs=haskell_builtin,
            artifact_dir=root / "trace-mismatch",
            require_haskell=True,
            require_equal=True,
        )
        summary, failed = run_comparison(trace_mismatch_args)
        assert failed, summary
        assert summary["trace_identity"]["violations"]
        assert "CEK flush trace_id" in summary["trace_identity"]["violations"][0]
        rust_cek.write_text(cek_flush, encoding="utf-8")

        missing_args = argparse.Namespace(
            rust_script_context=rust_script,
            haskell_script_context=None,
            rust_cek_flushes=rust_cek,
            haskell_cek_flushes=None,
            rust_builtin_costs=rust_builtin,
            haskell_builtin_costs=None,
            artifact_dir=root / "missing",
            require_haskell=True,
            require_equal=True,
        )
        try:
            build_cases(missing_args)
        except SystemExit as exc:
            assert "--haskell-script-context is required" in str(exc)
        else:
            raise AssertionError("expected require_haskell missing-log failure")

        missing_equal_args = argparse.Namespace(
            rust_script_context=rust_script,
            haskell_script_context=None,
            rust_cek_flushes=rust_cek,
            haskell_cek_flushes=None,
            rust_builtin_costs=rust_builtin,
            haskell_builtin_costs=None,
            artifact_dir=root / "missing-equal",
            require_haskell=False,
            require_equal=True,
            self_test=False,
        )
        try:
            validate_required_args(missing_equal_args)
        except SystemExit as exc:
            assert "--require-equal requires" in str(exc)
        else:
            raise AssertionError("expected require_equal missing-log failure")

    print("[ok] compare-gap-bp-traces self-test passed")
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run the complete Gap BP Plutus evidence comparison set"
    )
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--rust-script-context", type=Path)
    parser.add_argument("--haskell-script-context", type=Path)
    parser.add_argument("--rust-cek-flushes", type=Path)
    parser.add_argument("--haskell-cek-flushes", type=Path)
    parser.add_argument("--rust-builtin-costs", type=Path)
    parser.add_argument("--haskell-builtin-costs", type=Path)
    parser.add_argument(
        "--artifact-dir",
        type=Path,
        default=DEFAULT_ARTIFACT_DIR,
        help="Directory for aggregate and per-comparator summaries",
    )
    parser.add_argument(
        "--require-haskell",
        action="store_true",
        help="Require all three Haskell evidence logs before running comparisons",
    )
    parser.add_argument(
        "--require-equal",
        action="store_true",
        help="Exit non-zero when any supplied Haskell comparison differs",
    )
    args = parser.parse_args()
    validate_required_args(args, parser)
    return args


def main() -> int:
    args = parse_args()
    if args.self_test:
        return run_self_test()

    summary, failed = run_comparison(args)
    summary_path = args.artifact_dir / "summary.json"
    print(f"wrote {summary_path}")
    for result in summary["results"]:
        print(f"{result['name']}: {result['status']}")
        if result["status"] != "pass":
            if result["stdout"]:
                print(result["stdout"].strip())
            if result["stderr"]:
                print(result["stderr"].strip())
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
