#!/usr/bin/env python3
"""Validate final live core closeout evidence artifacts.

This gate is intentionally stricter than the local evidence self-test preflight:
it fails until the real Haskell/socket/operator artifacts for the open Gap BO,
Gap BP, R178, and BlockFetch closeout work are present under
`target/core-closeout/`.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import sys
import tempfile
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_ARTIFACT_ROOT = ROOT / "target" / "core-closeout"
GAP_BO_SLOT = 429460
R178_QUERIES = {"gov-state", "constitution", "committee-state"}
GAP_BO_HEX_KEYS = (
    "leader_seed",
    "nonce_seed",
    "leader_output",
    "nonce_output",
    "leader_proof_hash",
    "nonce_proof_hash",
)
GAP_BO_MIN_COMPARE_KEYS = (
    "classification",
    "first_slot",
    "d",
    "offset",
    "position",
    "asc_inv",
    "genesis_idx",
    "genesis_hash",
    "expected_delegate_hash",
    "actual_delegate_hash",
    "expected_vrf_key_hash",
    "actual_vrf_key_hash",
    "current_epoch",
    "epoch_nonce",
    "evolving_nonce",
    "candidate_nonce",
    "prev_hash_nonce",
    "lab_nonce",
    "nonce_state_phase",
    "epoch_nonce_hex",
    "evolving_nonce_hex",
    "candidate_nonce_hex",
    "prev_hash_nonce_hex",
    "lab_nonce_hex",
    "leader_seed",
    "nonce_seed",
    "leader_output",
    "nonce_output",
    "leader_proof_hash",
    "nonce_proof_hash",
)


def require_wsl_or_linux() -> None:
    if sys.platform == "win32":
        raise SystemExit(
            "check-core-closeout-artifacts.py must run under WSL/Linux; "
            "use `wsl -e bash -lc \"python3 scripts/check-core-closeout-artifacts.py\"`"
        )


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


def require_object(
    container: dict[str, Any],
    key: str,
    failures: list[str],
    prefix: str = "",
) -> dict[str, Any]:
    value = container.get(key)
    label = f"{prefix}.{key}" if prefix else key
    if not isinstance(value, dict):
        failures.append(f"{label} must be an object")
        return {}
    return value


def require_non_empty_list(
    container: dict[str, Any],
    key: str,
    failures: list[str],
    prefix: str = "",
) -> list[Any]:
    value = container.get(key)
    label = f"{prefix}.{key}" if prefix else key
    if not isinstance(value, list) or not value:
        failures.append(f"{label} must be a non-empty list")
        return []
    return value


def require_list(
    container: dict[str, Any],
    key: str,
    failures: list[str],
    prefix: str = "",
) -> list[Any]:
    value = container.get(key)
    label = f"{prefix}.{key}" if prefix else key
    if not isinstance(value, list):
        failures.append(f"{label} must be a list")
        return []
    return value


def require_generated_at(
    container: dict[str, Any],
    failures: list[str],
    prefix: str = "",
) -> None:
    value = container.get("generated_at_utc")
    label = f"{prefix}.generated_at_utc" if prefix else "generated_at_utc"
    if not isinstance(value, str) or not value:
        failures.append(f"{label} must be present")
        return
    try:
        dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        failures.append(f"{label} must be ISO-8601 parseable")


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


def check_hexish(
    fields: dict[str, Any],
    key: str,
    failures: list[str],
    prefix: str,
) -> None:
    value = fields.get(key)
    if not isinstance(value, str) or len(value) < 16:
        failures.append(f"{prefix}.{key} must look like live hex evidence")
        return
    try:
        int(value, 16)
    except ValueError:
        failures.append(f"{prefix}.{key} must be hex")


def artifact_result(name: str, path: Path, failures: list[str]) -> dict[str, Any]:
    return {
        "name": name,
        "path": str(path),
        "status": "fail" if failures else "pass",
        "failures": failures,
    }


def require_path(
    value: Any,
    failures: list[str],
    label: str,
) -> Path | None:
    if not isinstance(value, str) or not value:
        failures.append(f"{label} must be a non-empty path")
        return None
    if "self-test" in value:
        failures.append(f"{label} must not point at self-test data")
        return None
    return Path(value)


def require_existing_dir(
    value: Any,
    failures: list[str],
    label: str,
) -> Path | None:
    path = require_path(value, failures, label)
    if path is None:
        return None
    if not path.is_dir():
        failures.append(f"{label} must exist as a directory: {path}")
    return path


def require_existing_file(
    value: Any,
    failures: list[str],
    label: str,
) -> Path | None:
    path = require_path(value, failures, label)
    if path is None:
        return None
    if not path.is_file():
        failures.append(f"{label} must exist as a file: {path}")
    return path


def validate_gap_bo(path: Path) -> dict[str, Any]:
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
        if fixture.get("target_slot") != GAP_BO_SLOT:
            failures.append(f"target_slot must be {GAP_BO_SLOT}")
        if fixture.get("mismatches") != []:
            failures.append("mismatches must be empty")

        required_keys = require_non_empty_list(fixture, "required_keys", failures)
        compare_keys = require_non_empty_list(fixture, "compare_keys", failures)
        rust_fields = require_object(fixture, "rust_fields", failures)
        haskell_fields = require_object(fixture, "haskell_fields", failures)

        for key in (*GAP_BO_MIN_COMPARE_KEYS, *required_keys):
            if key not in rust_fields:
                failures.append(f"rust_fields.{key} must be present")
            if key not in haskell_fields:
                failures.append(f"haskell_fields.{key} must be present")
        missing_compare_keys = [
            key for key in GAP_BO_MIN_COMPARE_KEYS if key not in compare_keys
        ]
        if missing_compare_keys:
            failures.append(
                "compare_keys missing final Gap BO keys: "
                + ", ".join(missing_compare_keys)
            )
        if str(GAP_BO_SLOT) != rust_fields.get("slot"):
            failures.append("rust_fields.slot must match target_slot")
        if str(GAP_BO_SLOT) != haskell_fields.get("slot"):
            failures.append("haskell_fields.slot must match target_slot")
        for key in compare_keys:
            if rust_fields.get(key) != haskell_fields.get(key):
                failures.append(f"compared field {key} differs")
        for side, fields in (("rust_fields", rust_fields), ("haskell_fields", haskell_fields)):
            for key in GAP_BO_HEX_KEYS:
                check_hexish(fields, key, failures, side)

    return artifact_result("gap-bo", path, failures)


def validate_gap_bp(path: Path) -> dict[str, Any]:
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
        if not isinstance(expected_trace_id, str) or not expected_trace_id:
            failures.append("expected_trace_id must be present")
            expected_trace_id = ""
        parts = expected_trace_id.split(":")
        if len(parts) != 3 or len(parts[0]) < 16 or len(parts[1]) < 16:
            failures.append("expected_trace_id must be a live tx/script/version triple")
        if expected_trace_id == "aa:bb:V2":
            failures.append("expected_trace_id must not be the synthetic self-test id")

        trace_identity = require_object(fixture, "trace_identity", failures)
        if trace_identity.get("violations") != []:
            failures.append("trace_identity.violations must be empty")
        observed = require_object(trace_identity, "observed", failures, "trace_identity")
        if observed:
            for side in ("rust", "haskell"):
                side_observed = require_object(observed, side, failures, "observed")
                for surface, trace_id in side_observed.items():
                    if surface == "script_context":
                        matches = trace_id == expected_trace_id
                    else:
                        matches = trace_id == [expected_trace_id]
                    if not matches:
                        failures.append(
                            f"observed.{side}.{surface} must match expected_trace_id"
                        )

        script_context = require_object(fixture, "script_context", failures)
        comparison = require_object(script_context, "comparison", failures, "script_context")
        if comparison.get("byte_equal") is not True:
            failures.append("script_context.comparison.byte_equal must be true")
        for side in ("rust", "haskell"):
            side_context = require_object(script_context, side, failures, "script_context")
            metadata = require_object(side_context, "metadata", failures, f"script_context.{side}")
            if metadata.get("trace_id") != expected_trace_id:
                failures.append(f"script_context.{side}.metadata.trace_id must match")

        for child_key in ("cek_flushes", "builtin_costs"):
            child = require_object(fixture, child_key, failures)
            results = require_non_empty_list(child, "results", failures, child_key)
            for index, result in enumerate(results):
                if not isinstance(result, dict):
                    failures.append(f"{child_key}.results[{index}] must be an object")
                    continue
                if result.get("status") != "pass":
                    failures.append(f"{child_key}.results[{index}].status must be pass")
                for side in ("rust", "haskell"):
                    entry = require_object(
                        result,
                        side,
                        failures,
                        f"{child_key}.results[{index}]",
                    )
                    fields = require_object(
                        entry,
                        "fields",
                        failures,
                        f"{child_key}.results[{index}].{side}",
                    )
                    if fields.get("trace_id") != expected_trace_id:
                        failures.append(
                            f"{child_key}.results[{index}].{side}.trace_id must match"
                        )

    return artifact_result("gap-bp", path, failures)


def validate_r178(path: Path) -> dict[str, Any]:
    failures: list[str] = []
    fixture = load_json_object(path, failures)
    if fixture:
        if fixture.get("schema_version") != 1:
            failures.append("schema_version must be 1")
        if fixture.get("blocker") != "r178-conway-lsq":
            failures.append("blocker must be r178-conway-lsq")
        require_generated_at(fixture, failures)
        mode = require_object(fixture, "closeout_mode", failures)
        if fixture.get("status") != "pass":
            failures.append("status must be pass")
        byte_required = fixture.get("require_byte_equal") is True
        normalized_required = fixture.get("require_normalized_equal") is True
        if not byte_required and not normalized_required:
            failures.append("byte or normalized equality must be required")
        if mode.get("require_haskell") is not True:
            failures.append("closeout_mode.require_haskell must be true")
        if mode.get("require_byte_equal") is not byte_required:
            failures.append("closeout_mode.require_byte_equal must match fixture")
        if mode.get("require_normalized_equal") is not normalized_required:
            failures.append(
                "closeout_mode.require_normalized_equal must match fixture"
            )
        if not fixture.get("network_args"):
            failures.append("network_args must be present")

        cli_version = require_object(fixture, "cardano_cli_version", failures)
        if not str(cli_version.get("stdout", "")).startswith("cardano-cli "):
            failures.append("cardano_cli_version.stdout must identify cardano-cli")
        if not cli_version.get("stdout_sha256"):
            failures.append("cardano_cli_version.stdout_sha256 must be present")
        if "command" in cli_version:
            failures.append("cardano_cli_version must not include command")

        queries = require_object(fixture, "queries", failures)
        if set(queries) != R178_QUERIES:
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
            if not isinstance(normalized_json, str) or not normalized_json:
                failures.append(f"queries.{query}.normalized_json must be present")
            if normalized_json == '{"hash":"abc","slot":2}':
                failures.append(f"queries.{query}.normalized_json is self-test data")
            raw_stdout = require_object(
                result,
                "raw_stdout_comparison",
                failures,
                f"queries.{query}",
            )
            if byte_required and raw_stdout.get("byte_equal") is not True:
                failures.append(f"queries.{query}.raw_stdout bytes must match")
            raw_stderr = require_object(
                result,
                "raw_stderr_comparison",
                failures,
                f"queries.{query}",
            )
            if raw_stderr.get("byte_equal") is not True:
                failures.append(f"queries.{query}.raw_stderr bytes must match")
            for side in ("yggdrasil", "haskell"):
                side_result = require_object(result, side, failures, f"queries.{query}")
                if "command" in side_result:
                    failures.append(f"queries.{query}.{side} must not include command")
                if side_result.get("exit_code") != 0:
                    failures.append(f"queries.{query}.{side}.exit_code must be 0")
                if side_result.get("timed_out") is not False:
                    failures.append(f"queries.{query}.{side}.timed_out must be false")
                if normalized_required and side_result.get("normalized_json") != normalized_json:
                    failures.append(
                        f"queries.{query}.{side}.normalized_json must match"
                    )

    return artifact_result("r178", path, failures)


def validate_blockfetch(
    name: str,
    path: Path,
    expected_network: str,
    expected_magic: int,
    expected_knob: int,
    min_run_seconds: int,
) -> dict[str, Any]:
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
        if summary.get("network") != expected_network:
            failures.append(f"network must be {expected_network}")
        if summary.get("network_magic") != expected_magic:
            failures.append(f"network_magic must be {expected_magic}")

        worker = require_object(summary, "worker_assertions", failures)
        progress = require_object(summary, "progress_assertions", failures)
        tip = require_object(summary, "tip_comparison", failures)
        run = require_object(summary, "run", failures)
        artifacts = require_object(summary, "artifacts", failures)

        knob = numeric(worker.get("max_concurrent_block_fetch_peers"))
        expected_workers = numeric(worker.get("expected_workers"))
        if knob != expected_knob:
            failures.append(
                f"max_concurrent_block_fetch_peers must be {expected_knob}"
            )
        if expected_workers < expected_knob:
            failures.append("expected_workers must be at least the configured knob")
        if worker.get("require_workers") is not True:
            failures.append("require_workers must be true")
        if numeric(worker.get("workers_registered_max")) < expected_workers:
            failures.append("workers_registered_max must reach expected_workers")
        if numeric(worker.get("workers_registered_final")) < expected_workers:
            failures.append("workers_registered_final must reach expected_workers")
        if numeric(worker.get("workers_migrated_total")) < expected_workers:
            failures.append("workers_migrated_total must reach expected_workers")
        if numeric(worker.get("worker_shortfall_samples")) != 0:
            failures.append("worker_shortfall_samples must be 0")

        if progress.get("require_progress") is not True:
            failures.append("require_progress must be true")
        blocks = require_object(progress, "blocks_synced", failures, "progress_assertions")
        slot = require_object(progress, "current_slot", failures, "progress_assertions")
        if not (
            numeric(blocks.get("end")) > numeric(blocks.get("start"))
            or numeric(slot.get("end")) > numeric(slot.get("start"))
        ):
            failures.append("blocks_synced or current_slot must increase")

        if tip.get("require_tip_comparison") is not True:
            failures.append("require_tip_comparison must be true")
        if numeric(tip.get("min_tip_compare_passes")) < 2:
            failures.append("min_tip_compare_passes must be at least 2")
        if numeric(tip.get("tip_compare_passes")) < numeric(
            tip.get("min_tip_compare_passes")
        ):
            failures.append("tip_compare_passes must reach min_tip_compare_passes")
        logs = require_list(tip, "tip_compare_logs", failures, "tip_comparison")
        if numeric(tip.get("tip_compare_log_count")) < numeric(
            tip.get("tip_compare_passes")
        ):
            failures.append("tip_compare_log_count must reach tip_compare_passes")
        if len(logs) < numeric(tip.get("tip_compare_passes")):
            failures.append("tip_compare_logs must include every passing comparison")
        for index, log_path in enumerate(logs):
            require_existing_file(
                log_path,
                failures,
                f"tip_compare_logs[{index}]",
            )

        if numeric(run.get("run_seconds")) < min_run_seconds:
            failures.append(f"run_seconds must be at least {min_run_seconds}")
        if numeric(run.get("tip_query_timeout_seconds")) >= numeric(
            run.get("compare_interval_seconds")
        ):
            failures.append(
                "tip_query_timeout_seconds must be below compare_interval_seconds"
            )
        for key in ("run_dir", "log_dir", "metrics_dir", "tip_snapshots_dir"):
            require_existing_dir(
                artifacts.get(key),
                failures,
                f"artifacts.{key}",
            )
        for key in ("node_log", "summary_txt"):
            require_existing_file(
                artifacts.get(key),
                failures,
                f"artifacts.{key}",
            )

    return artifact_result(name, path, failures)


def expected_artifacts(root: Path) -> tuple[dict[str, Any], ...]:
    return (
        {
            "name": "gap-bo",
            "path": root / "gap-bo" / "fixture.json",
            "validator": validate_gap_bo,
        },
        {
            "name": "gap-bp",
            "path": root / "gap-bp" / "fixture.json",
            "validator": validate_gap_bp,
        },
        {
            "name": "r178",
            "path": root / "r178" / "fixture.json",
            "validator": validate_r178,
        },
        {
            "name": "blockfetch-preprod-two-peer",
            "path": root / "blockfetch" / "preprod-two-peer" / "summary.json",
            "validator": lambda path: validate_blockfetch(
                "blockfetch-preprod-two-peer",
                path,
                expected_network="preprod",
                expected_magic=1,
                expected_knob=2,
                min_run_seconds=600,
            ),
        },
        {
            "name": "blockfetch-preprod-knob4",
            "path": root / "blockfetch" / "preprod-knob4" / "summary.json",
            "validator": lambda path: validate_blockfetch(
                "blockfetch-preprod-knob4",
                path,
                expected_network="preprod",
                expected_magic=1,
                expected_knob=4,
                min_run_seconds=600,
            ),
        },
        {
            "name": "blockfetch-mainnet-24h",
            "path": root / "blockfetch" / "mainnet-24h" / "summary.json",
            "validator": lambda path: validate_blockfetch(
                "blockfetch-mainnet-24h",
                path,
                expected_network="mainnet",
                expected_magic=764824073,
                expected_knob=2,
                min_run_seconds=86_400,
            ),
        },
    )


def validate(root: Path) -> list[dict[str, Any]]:
    return [entry["validator"](entry["path"]) for entry in expected_artifacts(root)]


def write_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True), encoding="utf-8")


def generated_at() -> str:
    return dt.datetime.now(dt.UTC).isoformat()


def live_hex(seed: str) -> str:
    return (seed * 64)[:64]


def sample_gap_bo() -> dict[str, Any]:
    fields = {
        key: f"{key}-value"
        for key in ("slot", "era", "verification", *GAP_BO_MIN_COMPARE_KEYS)
    }
    fields.update(
        {
            "slot": str(GAP_BO_SLOT),
            "era": "Shelley",
            "verification": "ok",
            "leader_seed": live_hex("a"),
            "nonce_seed": live_hex("b"),
            "leader_output": live_hex("c"),
            "nonce_output": live_hex("d"),
            "leader_proof_hash": live_hex("e"),
            "nonce_proof_hash": live_hex("f"),
            "epoch_nonce_hex": live_hex("1"),
            "evolving_nonce_hex": "neutral",
            "candidate_nonce_hex": "neutral",
            "prev_hash_nonce_hex": "neutral",
            "lab_nonce_hex": "neutral",
        }
    )
    return {
        "schema_version": 1,
        "blocker": "gap-bo-tpraos-vrf",
        "generated_at_utc": generated_at(),
        "closeout_mode": {
            "require_haskell": True,
            "require_equal": True,
        },
        "target_slot": GAP_BO_SLOT,
        "status": "pass",
        "compare_keys": list(GAP_BO_MIN_COMPARE_KEYS),
        "required_keys": ["slot", "era", "verification", *GAP_BO_MIN_COMPARE_KEYS],
        "rust_fields": fields,
        "haskell_fields": dict(fields),
        "mismatches": [],
    }


def sample_gap_bp() -> dict[str, Any]:
    trace_id = f"{live_hex('1')}:{live_hex('2')}:V2"
    script_context = {
        "metadata": {"trace_id": trace_id},
        "comparison": {"byte_equal": True},
    }
    step_result = {
        "status": "pass",
        "rust": {"fields": {"trace_id": trace_id}},
        "haskell": {"fields": {"trace_id": trace_id}},
    }
    return {
        "schema_version": 1,
        "blocker": "gap-bp-plutus-v2-traces",
        "generated_at_utc": generated_at(),
        "closeout_mode": {
            "require_haskell": True,
            "require_equal": True,
        },
        "status": "pass",
        "expected_trace_id": trace_id,
        "trace_identity": {
            "violations": [],
            "observed": {
                "rust": {
                    "script_context": trace_id,
                    "cek_flushes": [trace_id],
                    "builtin_costs": [trace_id],
                },
                "haskell": {
                    "script_context": trace_id,
                    "cek_flushes": [trace_id],
                    "builtin_costs": [trace_id],
                },
            },
        },
        "script_context": {
            "comparison": {"byte_equal": True},
            "rust": script_context,
            "haskell": script_context,
        },
        "cek_flushes": {"results": [step_result]},
        "builtin_costs": {"results": [step_result]},
    }


def sample_r178() -> dict[str, Any]:
    normalized = '{"hash":"deadbeef","slot":123456}'
    side = {
        "exit_code": 0,
        "timed_out": False,
        "timeout_seconds": 60,
        "stdout_sha256": live_hex("a"),
        "stderr_sha256": live_hex("b"),
        "stdout_len": 33,
        "stderr_len": 0,
        "normalized_json": normalized,
        "normalized_json_sha256": live_hex("c"),
        "normalize_error": None,
    }
    query = {
        "status": "pass",
        "raw_stdout_comparison": {"byte_equal": False},
        "raw_stderr_comparison": {"byte_equal": True},
        "normalized_json": normalized,
        "yggdrasil": side,
        "haskell": side,
    }
    return {
        "schema_version": 1,
        "blocker": "r178-conway-lsq",
        "generated_at_utc": generated_at(),
        "closeout_mode": {
            "require_haskell": True,
            "require_byte_equal": False,
            "require_normalized_equal": True,
        },
        "status": "pass",
        "network_args": ["--testnet-magic", "1"],
        "require_byte_equal": False,
        "require_normalized_equal": True,
        "cardano_cli_version": {
            "exit_code": 0,
            "stdout": "cardano-cli 11.0.0.0\n",
            "stderr": "",
            "stdout_sha256": live_hex("d"),
            "stderr_sha256": live_hex("e"),
        },
        "queries": {name: dict(query) for name in R178_QUERIES},
    }


def sample_blockfetch(
    network: str,
    magic: int,
    knob: int,
    run_seconds: int,
    artifact_base: Path | None = None,
) -> dict[str, Any]:
    base = (
        artifact_base / f"{network}-{knob}"
        if artifact_base is not None
        else Path("/tmp/core-closeout") / f"{network}-{knob}"
    )
    log_dir = base / "logs"
    metrics_dir = base / "metrics"
    node_log = log_dir / "yggdrasil-node.log"
    summary_txt = log_dir / "summary.txt"
    tip_snapshots_dir = log_dir / "tip-snapshots"
    tip_compare_logs = [
        log_dir / "tip-compare-1.log",
        log_dir / "tip-compare-2.log",
    ]
    if artifact_base is not None:
        metrics_dir.mkdir(parents=True, exist_ok=True)
        tip_snapshots_dir.mkdir(parents=True, exist_ok=True)
        node_log.write_text("sample node log\n", encoding="utf-8")
        summary_txt.write_text("sample summary\n", encoding="utf-8")
        for index, log_path in enumerate(tip_compare_logs, start=1):
            log_path.write_text(
                f"sample tip comparison {index}\n",
                encoding="utf-8",
            )

    return {
        "schema_version": 1,
        "blocker": "blockfetch-section-6.5",
        "generated_at_utc": generated_at(),
        "status": "pass",
        "network": network,
        "network_magic": magic,
        "worker_assertions": {
            "max_concurrent_block_fetch_peers": knob,
            "expected_workers": knob,
            "require_workers": True,
            "workers_registered_max": knob,
            "workers_registered_final": knob,
            "workers_migrated_total": knob,
            "worker_shortfall_samples": 0,
        },
        "progress_assertions": {
            "require_progress": True,
            "blocks_synced": {"start": 1, "end": 2},
            "current_slot": {"start": 100, "end": 200},
        },
        "tip_comparison": {
            "require_tip_comparison": True,
            "min_tip_compare_passes": 2,
            "tip_compare_passes": 2,
            "tip_compare_log_count": 2,
            "tip_compare_logs": [str(path) for path in tip_compare_logs],
        },
        "run": {
            "run_seconds": run_seconds,
            "compare_interval_seconds": 60,
            "tip_query_timeout_seconds": 30,
        },
        "artifacts": {
            "run_dir": str(base),
            "log_dir": str(log_dir),
            "metrics_dir": str(metrics_dir),
            "node_log": str(node_log),
            "summary_txt": str(summary_txt),
            "tip_snapshots_dir": str(tip_snapshots_dir),
        },
    }


def write_sample_artifacts(root: Path) -> None:
    blockfetch_artifacts = root / "_blockfetch-artifacts"
    write_json(root / "gap-bo" / "fixture.json", sample_gap_bo())
    write_json(root / "gap-bp" / "fixture.json", sample_gap_bp())
    write_json(root / "r178" / "fixture.json", sample_r178())
    write_json(
        root / "blockfetch" / "preprod-two-peer" / "summary.json",
        sample_blockfetch("preprod", 1, 2, 600, blockfetch_artifacts),
    )
    write_json(
        root / "blockfetch" / "preprod-knob4" / "summary.json",
        sample_blockfetch("preprod", 1, 4, 600, blockfetch_artifacts),
    )
    write_json(
        root / "blockfetch" / "mainnet-24h" / "summary.json",
        sample_blockfetch("mainnet", 764824073, 2, 86_400, blockfetch_artifacts),
    )


def run_self_test() -> int:
    with tempfile.TemporaryDirectory(prefix="core-closeout-artifacts-") as tmp:
        root = Path(tmp)
        write_sample_artifacts(root)
        checks = validate(root)
        assert all(check["status"] == "pass" for check in checks), checks

        self_test_fixture = sample_gap_bp()
        self_test_fixture["expected_trace_id"] = "aa:bb:V2"
        write_json(root / "gap-bp" / "fixture.json", self_test_fixture)
        checks = validate(root)
        gap_bp = next(check for check in checks if check["name"] == "gap-bp")
        assert gap_bp["status"] == "fail"
        assert any("synthetic self-test id" in item for item in gap_bp["failures"])

        write_sample_artifacts(root)
        self_test_r178 = sample_r178()
        first_query = next(iter(self_test_r178["queries"].values()))
        first_query["normalized_json"] = '{"hash":"abc","slot":2}'
        write_json(root / "r178" / "fixture.json", self_test_r178)
        checks = validate(root)
        r178 = next(check for check in checks if check["name"] == "r178")
        assert r178["status"] == "fail"
        assert any("self-test data" in item for item in r178["failures"])

        write_sample_artifacts(root)
        blockfetch_path = root / "blockfetch" / "preprod-two-peer" / "summary.json"
        blockfetch_summary = load_json_object(blockfetch_path, [])
        first_log = Path(blockfetch_summary["tip_comparison"]["tip_compare_logs"][0])
        first_log.unlink()
        write_json(blockfetch_path, blockfetch_summary)
        checks = validate(root)
        blockfetch = next(
            check for check in checks if check["name"] == "blockfetch-preprod-two-peer"
        )
        assert blockfetch["status"] == "fail"
        assert any(
            "tip_compare_logs[0] must exist as a file" in item
            for item in blockfetch["failures"]
        )

    print("[ok] check-core-closeout-artifacts self-test passed")
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate final live core closeout evidence artifacts"
    )
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument(
        "--artifact-root",
        type=Path,
        default=DEFAULT_ARTIFACT_ROOT,
        help="Root containing final closeout artifacts",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    require_wsl_or_linux()
    if args.self_test:
        return run_self_test()

    checks = validate(args.artifact_root)
    failed = [check for check in checks if check["status"] != "pass"]
    summary = {
        "generated_at_utc": dt.datetime.now(dt.UTC).isoformat(),
        "status": "fail" if failed else "pass",
        "artifact_root": str(args.artifact_root),
        "checks": checks,
    }
    write_json(args.artifact_root / "summary.json", summary)
    for check in checks:
        print(f"[{check['status']}] {check['name']} {check['path']}")
        for failure in check["failures"]:
            print(f"  - {failure}")
    print(f"wrote {args.artifact_root / 'summary.json'}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
