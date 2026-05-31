#!/usr/bin/env python3
"""Compare Conway LocalStateQuery responses through upstream cardano-cli.

This is an operator evidence harness for the R178 follow-up.  It does not
decode Yggdrasil's internal Rust response directly; instead it drives the
official IntersectMBO cardano-cli against one or two node sockets.  A successful
upstream CLI decode proves the node returned the HFC QueryIfCurrent envelope
shape that cardano-cli expects.  When both Yggdrasil and Haskell sockets are
provided, the harness also records raw and normalized output hashes and can
require byte-for-byte equality.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CARDANO_CLI = (
    ROOT / ".reference-haskell-cardano-node" / "install" / "bin" / "cardano-cli"
)
NETWORK_MAGIC = {
    "mainnet": None,
    "preprod": 1,
    "preview": 2,
}
DEFAULT_QUERIES = ("gov-state", "constitution", "committee-state")


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def resolve_cardano_cli(value: str | None) -> str:
    if value:
        return value
    if DEFAULT_CARDANO_CLI.exists():
        return str(DEFAULT_CARDANO_CLI)
    found = shutil.which("cardano-cli")
    if found:
        return found
    raise SystemExit(
        "cardano-cli not found; pass --cardano-cli or run scripts/setup-reference.sh"
    )


def network_args(network: str, testnet_magic: int | None) -> list[str]:
    if testnet_magic is not None:
        return ["--testnet-magic", str(testnet_magic)]
    if network == "mainnet":
        return ["--mainnet"]
    return ["--testnet-magic", str(NETWORK_MAGIC[network])]


def normalize_json(data: bytes) -> tuple[str | None, str | None]:
    try:
        parsed: Any = json.loads(data.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        return None, str(exc)
    normalized = json.dumps(parsed, sort_keys=True, separators=(",", ":"))
    return normalized, None


def first_diff(left: bytes, right: bytes) -> int | None:
    for offset, (left_byte, right_byte) in enumerate(zip(left, right)):
        if left_byte != right_byte:
            return offset
    if len(left) != len(right):
        return min(len(left), len(right))
    return None


def diff_window(left: bytes, right: bytes, offset: int, radius: int = 32) -> dict[str, Any]:
    start = max(0, offset - radius)
    end = min(max(len(left), len(right)), offset + radius)
    return {
        "offset": offset,
        "start": start,
        "end": end,
        "yggdrasil_hex": left[start:min(end, len(left))].hex(),
        "haskell_hex": right[start:min(end, len(right))].hex(),
    }


def compare_raw_bytes(left: bytes, right: bytes) -> dict[str, Any]:
    offset = first_diff(left, right)
    equal = offset is None
    return {
        "byte_equal": equal,
        "first_diff_offset": offset,
        "length_delta": len(left) - len(right),
        "diff_window": None if equal else diff_window(left, right, offset),
    }


def cardano_cli_version(cardano_cli: str) -> dict[str, Any]:
    command = [cardano_cli, "--version"]
    proc = subprocess.run(command, capture_output=True, check=False)
    result = {
        "command": command,
        "exit_code": proc.returncode,
        "stdout": proc.stdout.decode("utf-8", errors="replace"),
        "stderr": proc.stderr.decode("utf-8", errors="replace"),
        "stdout_sha256": sha256_bytes(proc.stdout),
        "stderr_sha256": sha256_bytes(proc.stderr),
    }
    if proc.returncode != 0:
        raise SystemExit(
            "cardano-cli --version failed; pass --cardano-cli for a runnable "
            "upstream binary or run under WSL/Linux with the reference install"
        )
    return result


def run_query(
    cardano_cli: str,
    socket_path: Path,
    query: str,
    net_args: list[str],
) -> dict[str, Any]:
    command = [
        cardano_cli,
        "conway",
        "query",
        query,
        *net_args,
        "--socket-path",
        str(socket_path),
        "--volatile-tip",
        "--output-json",
    ]
    proc = subprocess.run(command, capture_output=True, check=False)
    normalized, normalize_error = normalize_json(proc.stdout)
    return {
        "command": command,
        "exit_code": proc.returncode,
        "stdout_sha256": sha256_bytes(proc.stdout),
        "stderr_sha256": sha256_bytes(proc.stderr),
        "stdout_len": len(proc.stdout),
        "stderr_len": len(proc.stderr),
        "stdout": proc.stdout.decode("utf-8", errors="replace"),
        "stderr": proc.stderr.decode("utf-8", errors="replace"),
        "_stdout_bytes": proc.stdout,
        "_stderr_bytes": proc.stderr,
        "normalized_json": normalized,
        "normalized_json_sha256": sha256_bytes(normalized.encode("utf-8"))
        if normalized is not None
        else None,
        "normalize_error": normalize_error,
    }


def compare_results(
    ygg: dict[str, Any],
    haskell: dict[str, Any] | None,
    require_byte_equal: bool,
    require_normalized_equal: bool,
) -> tuple[str, list[str]]:
    failures: list[str] = []
    if ygg["exit_code"] != 0:
        failures.append("yggdrasil cardano-cli query exited non-zero")
    if ygg["normalize_error"] is not None:
        failures.append("yggdrasil stdout was not valid JSON")

    if haskell is None:
        return ("pass" if not failures else "fail"), failures

    if haskell["exit_code"] != 0:
        failures.append("haskell cardano-cli query exited non-zero")
    if haskell["normalize_error"] is not None:
        failures.append("haskell stdout was not valid JSON")

    if require_byte_equal and ygg["stdout_sha256"] != haskell["stdout_sha256"]:
        failures.append("raw stdout bytes differ")
    if (
        require_normalized_equal
        and ygg["normalized_json_sha256"] != haskell["normalized_json_sha256"]
    ):
        failures.append("normalized JSON differs")

    return ("pass" if not failures else "fail"), failures


def json_ready_result(result: dict[str, Any]) -> dict[str, Any]:
    return {key: value for key, value in result.items() if not key.startswith("_")}


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def write_bytes(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def write_query_artifacts(
    artifact_dir: Path,
    query: str,
    label: str,
    result: dict[str, Any],
) -> None:
    write_bytes(artifact_dir / f"{query}.{label}.stdout.bin", result["_stdout_bytes"])
    write_bytes(artifact_dir / f"{query}.{label}.stderr.bin", result["_stderr_bytes"])
    write_text(artifact_dir / f"{query}.{label}.stdout", result["stdout"])
    write_text(artifact_dir / f"{query}.{label}.stderr", result["stderr"])


def validate_required_args(
    args: argparse.Namespace,
    parser: argparse.ArgumentParser | None = None,
) -> None:
    def fail(message: str) -> None:
        if parser is not None:
            parser.error(message)
        raise SystemExit(message)

    if not args.self_test and args.ygg_socket is None:
        fail("--ygg-socket is required unless --self-test is set")
    if not args.self_test and args.require_haskell and args.haskell_socket is None:
        fail("--haskell-socket is required with --require-haskell")
    if (
        not args.self_test
        and (args.require_byte_equal or args.require_normalized_equal)
        and args.haskell_socket is None
    ):
        fail(
            "--haskell-socket is required with --require-byte-equal "
            "or --require-normalized-equal"
        )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Compare Conway LSQ responses through upstream cardano-cli"
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="Run local parser/comparison checks without sockets or cardano-cli",
    )
    parser.add_argument("--ygg-socket", type=Path)
    parser.add_argument("--haskell-socket", type=Path)
    parser.add_argument("--cardano-cli")
    parser.add_argument(
        "--network",
        choices=sorted(NETWORK_MAGIC),
        default="preview",
        help="Network preset used when --testnet-magic is not supplied",
    )
    parser.add_argument("--testnet-magic", type=int)
    parser.add_argument(
        "--query",
        action="append",
        choices=DEFAULT_QUERIES,
        help="Conway query to run; repeatable. Defaults to gov-state, constitution, committee-state",
    )
    parser.add_argument(
        "--artifact-dir",
        type=Path,
        default=Path(os.environ.get("ARTIFACT_DIR", "target/conway-lsq-comparison")),
    )
    parser.add_argument("--require-byte-equal", action="store_true")
    parser.add_argument("--require-normalized-equal", action="store_true")
    parser.add_argument(
        "--require-haskell",
        action="store_true",
        help="Require --haskell-socket before running a closeout comparison",
    )
    args = parser.parse_args()
    validate_required_args(args, parser)
    return args


def sample_result(
    *,
    exit_code: int = 0,
    stdout: bytes = b'{"slot":2,"hash":"abc"}\n',
    stderr: bytes = b"",
) -> dict[str, Any]:
    normalized, normalize_error = normalize_json(stdout)
    return {
        "command": ["cardano-cli", "conway", "query", "gov-state"],
        "exit_code": exit_code,
        "stdout_sha256": sha256_bytes(stdout),
        "stderr_sha256": sha256_bytes(stderr),
        "stdout_len": len(stdout),
        "stderr_len": len(stderr),
        "stdout": stdout.decode("utf-8", errors="replace"),
        "stderr": stderr.decode("utf-8", errors="replace"),
        "_stdout_bytes": stdout,
        "_stderr_bytes": stderr,
        "normalized_json": normalized,
        "normalized_json_sha256": sha256_bytes(normalized.encode("utf-8"))
        if normalized is not None
        else None,
        "normalize_error": normalize_error,
    }


def run_self_test() -> int:
    assert network_args("mainnet", None) == ["--mainnet"]
    assert network_args("preprod", None) == ["--testnet-magic", "1"]
    assert network_args("preview", 42) == ["--testnet-magic", "42"]

    # HFC QueryIfCurrent result envelopes per upstream encodeEitherMismatch:
    # Right/match = [body], Left/mismatch = [requestedEra, ledgerEra].
    assert bytes([0x81, 0x91, 0x01]) == bytes.fromhex("819101")
    assert bytes.fromhex("8282056742616262616765820666436f6e776179") == bytes(
        [
            0x82,
            0x82,
            0x05,
            0x67,
            *b"Babbage",
            0x82,
            0x06,
            0x66,
            *b"Conway",
        ]
    )

    normalized, error = normalize_json(b'{"b":2,"a":1}')
    assert normalized == '{"a":1,"b":2}'
    assert error is None
    normalized, error = normalize_json(b"not-json")
    assert normalized is None
    assert error is not None

    raw_comparison = compare_raw_bytes(b'{"a":1}\n', b'{\n  "a": 1\n}\n')
    assert not raw_comparison["byte_equal"]
    assert raw_comparison["first_diff_offset"] == 1
    assert raw_comparison["diff_window"] is not None
    same_raw_comparison = compare_raw_bytes(b"abc", b"abc")
    assert same_raw_comparison["byte_equal"]
    assert same_raw_comparison["diff_window"] is None

    ygg = sample_result(stdout=b'{"slot":2,"hash":"abc"}\n')
    haskell_same_json = sample_result(stdout=b'{\n  "hash": "abc",\n  "slot": 2\n}\n')
    status, failures = compare_results(ygg, None, False, False)
    assert status == "pass", failures
    status, failures = compare_results(ygg, haskell_same_json, False, True)
    assert status == "pass", failures
    status, failures = compare_results(ygg, haskell_same_json, True, False)
    assert status == "fail"
    assert "raw stdout bytes differ" in failures

    bad = sample_result(exit_code=1, stdout=b"not-json", stderr=b"boom")
    status, failures = compare_results(bad, None, False, False)
    assert status == "fail"
    assert "yggdrasil cardano-cli query exited non-zero" in failures
    assert "yggdrasil stdout was not valid JSON" in failures

    try:
        validate_required_args(
            argparse.Namespace(
                self_test=False,
                ygg_socket=Path("node.sock"),
                haskell_socket=None,
                require_haskell=True,
                require_byte_equal=False,
                require_normalized_equal=False,
            )
        )
    except SystemExit as exc:
        assert "--haskell-socket is required" in str(exc)
    else:
        raise AssertionError("expected --require-haskell to reject missing socket")

    try:
        validate_required_args(
            argparse.Namespace(
                self_test=False,
                ygg_socket=Path("node.sock"),
                haskell_socket=None,
                require_haskell=False,
                require_byte_equal=True,
                require_normalized_equal=False,
            )
        )
    except SystemExit as exc:
        assert "--haskell-socket is required" in str(exc)
    else:
        raise AssertionError("expected equality mode to reject missing socket")

    print("[ok] compare-conway-lsq self-test passed")
    return 0


def main() -> int:
    args = parse_args()
    if args.self_test:
        return run_self_test()

    cardano_cli = resolve_cardano_cli(args.cardano_cli)
    queries = tuple(args.query or DEFAULT_QUERIES)
    net_args = network_args(args.network, args.testnet_magic)
    artifact_dir = args.artifact_dir
    artifact_dir.mkdir(parents=True, exist_ok=True)
    cli_version = cardano_cli_version(cardano_cli)

    summary: dict[str, Any] = {
        "cardano_cli": cardano_cli,
        "cardano_cli_version": cli_version,
        "network_args": net_args,
        "ygg_socket": str(args.ygg_socket),
        "haskell_socket": str(args.haskell_socket) if args.haskell_socket else None,
        "require_haskell": args.require_haskell,
        "require_byte_equal": args.require_byte_equal,
        "require_normalized_equal": args.require_normalized_equal,
        "queries": {},
    }

    failed = False
    for query in queries:
        ygg = run_query(cardano_cli, args.ygg_socket, query, net_args)
        haskell = (
            run_query(cardano_cli, args.haskell_socket, query, net_args)
            if args.haskell_socket
            else None
        )
        status, failures = compare_results(
            ygg,
            haskell,
            args.require_byte_equal,
            args.require_normalized_equal,
        )
        failed = failed or status == "fail"
        stdout_comparison = (
            compare_raw_bytes(ygg["_stdout_bytes"], haskell["_stdout_bytes"])
            if haskell is not None
            else None
        )
        stderr_comparison = (
            compare_raw_bytes(ygg["_stderr_bytes"], haskell["_stderr_bytes"])
            if haskell is not None
            else None
        )
        summary["queries"][query] = {
            "status": status,
            "failures": failures,
            "raw_stdout_comparison": stdout_comparison,
            "raw_stderr_comparison": stderr_comparison,
            "yggdrasil": json_ready_result(ygg),
            "haskell": json_ready_result(haskell) if haskell is not None else None,
        }
        write_query_artifacts(artifact_dir, query, "ygg", ygg)
        if haskell is not None:
            write_query_artifacts(artifact_dir, query, "haskell", haskell)

    summary_path = artifact_dir / "summary.json"
    summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True), encoding="utf-8")
    print(f"wrote {summary_path}")
    for query, result in summary["queries"].items():
        print(f"{query}: {result['status']}")
        for failure in result["failures"]:
            print(f"  - {failure}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
