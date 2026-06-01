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


def stage_one(label: str, source: Path, root: Path, force: bool) -> dict[str, str]:
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
    staged = [
        stage_one(label, sources[label], root, force)
        for label in DESTINATIONS
    ]
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

        expect_system_exit(
            lambda: stage_artifacts(sources, dst, force=False),
            "destination already exists",
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
