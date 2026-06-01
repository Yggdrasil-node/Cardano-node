#!/usr/bin/env python3
"""Stage final live core closeout artifacts into the canonical layout.

This helper does not produce parity evidence by itself. It copies artifacts
already produced by the strict Gap BO, Gap BP, R178, and BlockFetch closeout
runs into `target/core-closeout/`, then invokes
`check-core-closeout-artifacts.py` so weak or misplaced artifacts fail before
any status document can cite them.
"""

from __future__ import annotations

import argparse
import datetime as dt
import importlib.util
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_ARTIFACT_ROOT = ROOT / "target" / "core-closeout"
VALIDATOR = ROOT / "scripts" / "check-core-closeout-artifacts.py"

DESTINATIONS = {
    "gap_bo_fixture": Path("gap-bo/fixture.json"),
    "gap_bp_fixture": Path("gap-bp/fixture.json"),
    "r178_fixture": Path("r178/fixture.json"),
    "blockfetch_preprod_two_peer": Path("blockfetch/preprod-two-peer/summary.json"),
    "blockfetch_preprod_knob4": Path("blockfetch/preprod-knob4/summary.json"),
    "blockfetch_mainnet_24h": Path("blockfetch/mainnet-24h/summary.json"),
}
BLOCKFETCH_LABELS = frozenset(
    {
        "blockfetch_preprod_two_peer",
        "blockfetch_preprod_knob4",
        "blockfetch_mainnet_24h",
    }
)


def require_wsl_or_linux() -> None:
    if sys.platform == "win32":
        raise SystemExit(
            "stage-core-closeout-artifacts.py must run under WSL/Linux; "
            "use `wsl -e bash -lc \"python3 scripts/stage-core-closeout-artifacts.py ...\"`"
        )


def load_validator_module() -> Any:
    spec = importlib.util.spec_from_file_location("core_closeout_validator", VALIDATOR)
    if spec is None or spec.loader is None:
        raise SystemExit(f"failed to load validator module from {VALIDATOR}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def source_map(args: argparse.Namespace) -> dict[str, Path]:
    return {
        key: value
        for key, value in {
            "gap_bo_fixture": args.gap_bo_fixture,
            "gap_bp_fixture": args.gap_bp_fixture,
            "r178_fixture": args.r178_fixture,
            "blockfetch_preprod_two_peer": args.blockfetch_preprod_two_peer,
            "blockfetch_preprod_knob4": args.blockfetch_preprod_knob4,
            "blockfetch_mainnet_24h": args.blockfetch_mainnet_24h,
        }.items()
        if value is not None
    }


def validate_sources(sources: dict[str, Path]) -> None:
    missing_args = [key for key in DESTINATIONS if key not in sources]
    if missing_args:
        joined = ", ".join("--" + key.replace("_", "-") for key in missing_args)
        raise SystemExit(f"missing required closeout artifact inputs: {joined}")
    for key, path in sources.items():
        if not path.is_file():
            flag = "--" + key.replace("_", "-")
            raise SystemExit(f"{flag} does not point at a file: {path}")


def load_json_object(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise SystemExit(f"failed to parse {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise SystemExit(f"{path} did not contain a JSON object")
    return value


def require_object(container: dict[str, Any], key: str, label: str) -> dict[str, Any]:
    value = container.get(key)
    if not isinstance(value, dict):
        raise SystemExit(f"{label}.{key} must be an object")
    return value


def require_list(container: dict[str, Any], key: str, label: str) -> list[Any]:
    value = container.get(key)
    if not isinstance(value, list):
        raise SystemExit(f"{label}.{key} must be a list")
    return value


def require_existing_path(value: Any, label: str, *, kind: str) -> Path:
    if not isinstance(value, str) or not value:
        raise SystemExit(f"{label} must be a non-empty path")
    path = Path(value).expanduser()
    if kind == "file" and not path.is_file():
        raise SystemExit(f"{label} must exist as a file: {path}")
    if kind == "dir" and not path.is_dir():
        raise SystemExit(f"{label} must exist as a directory: {path}")
    return path


def ensure_inside_root(path: Path, root: Path) -> None:
    root_resolved = root.resolve()
    path_resolved = path.resolve()
    if path_resolved == root_resolved or root_resolved not in path_resolved.parents:
        raise SystemExit(f"refusing to replace path outside artifact root: {path}")


def reset_directory(path: Path, root: Path, force: bool) -> None:
    if path.exists():
        if not force:
            raise SystemExit(
                f"destination already exists: {path}; pass --force to replace it"
            )
        ensure_inside_root(path, root)
        if path.is_dir():
            shutil.rmtree(path)
        else:
            path.unlink()
    path.mkdir(parents=True, exist_ok=True)


def maybe_mapped_path(source: Path, source_root: Path, staged_root: Path) -> Path | None:
    try:
        return staged_root / source.resolve().relative_to(source_root.resolve())
    except ValueError:
        return None


def unique_file_path(directory: Path, source: Path, used: set[Path]) -> Path:
    name = source.name or "artifact"
    candidate = directory / name
    if candidate not in used and not candidate.exists():
        used.add(candidate)
        return candidate
    stem = source.stem or "artifact"
    suffix = source.suffix
    index = 1
    while True:
        candidate = directory / f"{stem}-{index}{suffix}"
        if candidate not in used and not candidate.exists():
            used.add(candidate)
            return candidate
        index += 1


def stage_referenced_file(
    source: Path,
    source_log_dir: Path,
    staged_log_dir: Path,
    fallback_dir: Path,
    used: set[Path],
) -> Path:
    mapped = maybe_mapped_path(source, source_log_dir, staged_log_dir)
    if mapped is not None:
        if not mapped.is_file():
            raise SystemExit(f"staged file missing after log copy: {mapped}")
        return mapped

    fallback_dir.mkdir(parents=True, exist_ok=True)
    destination = unique_file_path(fallback_dir, source, used)
    shutil.copy2(source, destination)
    return destination


def stage_referenced_dir(
    source: Path,
    source_log_dir: Path,
    staged_log_dir: Path,
    fallback_dir: Path,
) -> Path:
    mapped = maybe_mapped_path(source, source_log_dir, staged_log_dir)
    if mapped is not None:
        if not mapped.is_dir():
            raise SystemExit(f"staged directory missing after log copy: {mapped}")
        return mapped

    if fallback_dir.exists():
        shutil.rmtree(fallback_dir)
    shutil.copytree(source, fallback_dir)
    return fallback_dir


def stage_one(label: str, source: Path, root: Path, force: bool) -> dict[str, Any]:
    destination = root / DESTINATIONS[label]
    if destination.exists() and not force:
        raise SystemExit(
            f"destination already exists: {destination}; pass --force to replace it"
        )
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)
    return {
        "name": label,
        "source": str(source),
        "destination": str(destination),
    }


def stage_blockfetch(
    label: str,
    source: Path,
    root: Path,
    force: bool,
) -> dict[str, Any]:
    destination = root / DESTINATIONS[label]
    if destination.exists() and not force:
        raise SystemExit(
            f"destination already exists: {destination}; pass --force to replace it"
        )

    summary = load_json_object(source)
    artifacts = require_object(summary, "artifacts", label)
    tip_comparison = require_object(summary, "tip_comparison", label)
    tip_compare_logs = require_list(
        tip_comparison,
        "tip_compare_logs",
        f"{label}.tip_comparison",
    )

    source_log_dir = require_existing_path(
        artifacts.get("log_dir"),
        f"{label}.artifacts.log_dir",
        kind="dir",
    )
    source_metrics_dir = require_existing_path(
        artifacts.get("metrics_dir"),
        f"{label}.artifacts.metrics_dir",
        kind="dir",
    )
    source_tip_snapshots_dir = require_existing_path(
        artifacts.get("tip_snapshots_dir"),
        f"{label}.artifacts.tip_snapshots_dir",
        kind="dir",
    )
    source_node_log = require_existing_path(
        artifacts.get("node_log"),
        f"{label}.artifacts.node_log",
        kind="file",
    )
    source_summary_txt = require_existing_path(
        artifacts.get("summary_txt"),
        f"{label}.artifacts.summary_txt",
        kind="file",
    )
    source_tip_logs = [
        require_existing_path(
            log_path,
            f"{label}.tip_comparison.tip_compare_logs[{index}]",
            kind="file",
        )
        for index, log_path in enumerate(tip_compare_logs)
    ]

    artifact_root = destination.parent / "artifacts"
    reset_directory(artifact_root, root, force)
    destination.parent.mkdir(parents=True, exist_ok=True)
    staged_log_dir = artifact_root / "logs"
    staged_metrics_dir = artifact_root / "metrics"
    shutil.copytree(source_log_dir, staged_log_dir)
    shutil.copytree(source_metrics_dir, staged_metrics_dir)

    fallback_files = artifact_root / "files"
    used_fallback_files: set[Path] = set()
    staged_node_log = stage_referenced_file(
        source_node_log,
        source_log_dir,
        staged_log_dir,
        fallback_files,
        used_fallback_files,
    )
    staged_summary_txt = stage_referenced_file(
        source_summary_txt,
        source_log_dir,
        staged_log_dir,
        fallback_files,
        used_fallback_files,
    )
    staged_tip_logs = [
        stage_referenced_file(
            source_log,
            source_log_dir,
            staged_log_dir,
            fallback_files,
            used_fallback_files,
        )
        for source_log in source_tip_logs
    ]
    staged_tip_snapshots_dir = stage_referenced_dir(
        source_tip_snapshots_dir,
        source_log_dir,
        staged_log_dir,
        artifact_root / "tip-snapshots",
    )

    original_artifacts = dict(artifacts)
    original_tip_logs = list(tip_compare_logs)
    artifacts.update(
        {
            "run_dir": str(artifact_root),
            "log_dir": str(staged_log_dir),
            "metrics_dir": str(staged_metrics_dir),
            "node_log": str(staged_node_log),
            "summary_txt": str(staged_summary_txt),
            "tip_snapshots_dir": str(staged_tip_snapshots_dir),
        }
    )
    tip_comparison["tip_compare_logs"] = [str(path) for path in staged_tip_logs]
    summary["closeout_staging"] = {
        "staged_at_utc": dt.datetime.now(dt.UTC).isoformat(),
        "source_summary_json": str(source),
        "staged_artifact_root": str(artifact_root),
        "source_artifacts": original_artifacts,
        "source_tip_compare_logs": original_tip_logs,
    }
    write_json(destination, summary)
    return {
        "name": label,
        "source": str(source),
        "destination": str(destination),
        "artifact_root": str(artifact_root),
        "staged_tip_compare_logs": len(staged_tip_logs),
    }


def run_final_check(root: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            sys.executable,
            str(VALIDATOR),
            "--artifact-root",
            str(root),
        ],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )


def write_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True), encoding="utf-8")


def stage_artifacts(
    sources: dict[str, Path],
    root: Path,
    *,
    force: bool,
) -> tuple[dict[str, Any], int]:
    validate_sources(sources)
    staged = []
    for label in DESTINATIONS:
        if label in BLOCKFETCH_LABELS:
            staged.append(stage_blockfetch(label, sources[label], root, force))
        else:
            staged.append(stage_one(label, sources[label], root, force))
    final = run_final_check(root)
    summary = {
        "generated_at_utc": dt.datetime.now(dt.UTC).isoformat(),
        "artifact_root": str(root),
        "status": "pass" if final.returncode == 0 else "fail",
        "staged": staged,
        "final_check": {
            "command": final.args,
            "exit_code": final.returncode,
            "stdout": final.stdout,
            "stderr": final.stderr,
        },
    }
    write_json(root / "staging-summary.json", summary)
    return summary, final.returncode


def sample_sources(root: Path) -> dict[str, Path]:
    return {
        "gap_bo_fixture": root / "gap-bo" / "fixture.json",
        "gap_bp_fixture": root / "gap-bp" / "fixture.json",
        "r178_fixture": root / "r178" / "fixture.json",
        "blockfetch_preprod_two_peer": root
        / "blockfetch"
        / "preprod-two-peer"
        / "summary.json",
        "blockfetch_preprod_knob4": root
        / "blockfetch"
        / "preprod-knob4"
        / "summary.json",
        "blockfetch_mainnet_24h": root
        / "blockfetch"
        / "mainnet-24h"
        / "summary.json",
    }


def expect_system_exit(action: Any, needle: str) -> None:
    try:
        action()
    except SystemExit as exc:
        if needle not in str(exc):
            raise AssertionError(f"expected {needle!r} in {exc!r}") from exc
    else:
        raise AssertionError(f"expected SystemExit containing {needle!r}")


def run_self_test() -> int:
    validator = load_validator_module()
    with tempfile.TemporaryDirectory(prefix="core-closeout-stage-") as tmp:
        root = Path(tmp)
        src = root / "src"
        dst = root / "dst"
        validator.write_sample_artifacts(src)
        sources = sample_sources(src)

        summary, code = stage_artifacts(sources, dst, force=False)
        assert code == 0, summary
        assert summary["status"] == "pass"
        assert (dst / "gap-bo" / "fixture.json").is_file()
        assert (dst / "staging-summary.json").is_file()
        preprod_summary_path = dst / "blockfetch" / "preprod-two-peer" / "summary.json"
        preprod_summary = load_json_object(preprod_summary_path)
        staged_run_dir = Path(preprod_summary["artifacts"]["run_dir"])
        assert staged_run_dir == dst / "blockfetch" / "preprod-two-peer" / "artifacts"
        assert Path(preprod_summary["artifacts"]["log_dir"]).is_dir()
        assert Path(preprod_summary["artifacts"]["metrics_dir"]).is_dir()
        assert Path(preprod_summary["artifacts"]["node_log"]).is_file()
        assert Path(preprod_summary["artifacts"]["summary_txt"]).is_file()
        assert Path(preprod_summary["artifacts"]["tip_snapshots_dir"]).is_dir()
        assert all(
            Path(path).is_file()
            for path in preprod_summary["tip_comparison"]["tip_compare_logs"]
        )

        shutil.rmtree(src / "_blockfetch-artifacts")
        checks = validator.validate(dst)
        assert all(check["status"] == "pass" for check in checks), checks

        expect_system_exit(
            lambda: stage_artifacts(sources, dst, force=False),
            "destination already exists",
        )

        missing_artifact_src = root / "missing-artifact-src"
        validator.write_sample_artifacts(missing_artifact_src)
        missing_sources = sample_sources(missing_artifact_src)
        missing_summary_path = missing_sources["blockfetch_preprod_two_peer"]
        missing_summary = load_json_object(missing_summary_path)
        first_tip_log = Path(missing_summary["tip_comparison"]["tip_compare_logs"][0])
        first_tip_log.unlink()
        write_json(missing_summary_path, missing_summary)
        expect_system_exit(
            lambda: stage_artifacts(
                missing_sources,
                root / "missing-artifact-dst",
                force=False,
            ),
            "tip_compare_logs[0] must exist as a file",
        )

        invalid_src = root / "invalid-src"
        validator.write_sample_artifacts(invalid_src)
        write_json(invalid_src / "gap-bp" / "fixture.json", {"status": "pass"})
        invalid_summary, invalid_code = stage_artifacts(
            sample_sources(invalid_src),
            root / "invalid-dst",
            force=False,
        )
        assert invalid_code != 0, invalid_summary
        assert invalid_summary["status"] == "fail"
        assert "gap-bp" in invalid_summary["final_check"]["stdout"]

    print("[ok] stage-core-closeout-artifacts self-test passed")
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Stage strict live core closeout artifacts and run the final gate"
    )
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument(
        "--artifact-root",
        type=Path,
        default=DEFAULT_ARTIFACT_ROOT,
        help="Canonical closeout artifact root to populate",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="Replace existing staged artifacts",
    )
    parser.add_argument("--gap-bo-fixture", type=Path)
    parser.add_argument("--gap-bp-fixture", type=Path)
    parser.add_argument("--r178-fixture", type=Path)
    parser.add_argument("--blockfetch-preprod-two-peer", type=Path)
    parser.add_argument("--blockfetch-preprod-knob4", type=Path)
    parser.add_argument("--blockfetch-mainnet-24h", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    require_wsl_or_linux()
    if args.self_test:
        return run_self_test()

    summary, code = stage_artifacts(
        source_map(args),
        args.artifact_root,
        force=args.force,
    )
    print(f"wrote {args.artifact_root / 'staging-summary.json'}")
    print(summary["final_check"]["stdout"], end="")
    if summary["final_check"]["stderr"]:
        print(summary["final_check"]["stderr"], file=sys.stderr, end="")
    return code


if __name__ == "__main__":
    sys.exit(main())
