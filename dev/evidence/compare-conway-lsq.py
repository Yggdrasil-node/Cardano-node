#!/usr/bin/env python3
"""Compare Conway LocalStateQuery responses through upstream cardano-cli.

This is an operator evidence harness for the R178 follow-up.  It does not
decode Yggdrasil's internal Rust response directly; instead it drives the
official IntersectMBO cardano-cli against one or two node sockets.  A successful
upstream CLI decode proves the node returned the HFC QueryIfCurrent envelope
shape that cardano-cli expects.  When both Yggdrasil and Haskell sockets are
provided, the harness also records raw and normalized output hashes and can
require byte-for-byte equality. Use `--write-fixture <path>` only with strict
Haskell equality closeout mode to persist a normalized regression fixture.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from collections.abc import Callable
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_CARDANO_CLI = (
    ROOT / ".reference-haskell-cardano-node" / "install" / "bin" / "cardano-cli"
)
DEFAULT_TIMEOUT_SECONDS = 60.0
SELF_TEST_FIXTURE = ROOT / "target" / "r178-conway-lsq-self-test" / "fixture.json"
NETWORK_MAGIC = {
    "mainnet": None,
    "preprod": 1,
    "preview": 2,
}
DEFAULT_QUERIES = ("gov-state", "constitution", "committee-state")


def parse_timeout_seconds(raw: str) -> float:
    try:
        value = float(raw)
    except ValueError as exc:
        raise argparse.ArgumentTypeError("must be a positive number") from exc
    if value <= 0:
        raise argparse.ArgumentTypeError("must be a positive number")
    return value


def default_timeout_seconds() -> float:
    raw = os.environ.get("CARDANO_CLI_TIMEOUT_SECONDS", str(DEFAULT_TIMEOUT_SECONDS))
    try:
        return parse_timeout_seconds(raw)
    except argparse.ArgumentTypeError as exc:
        raise SystemExit(f"CARDANO_CLI_TIMEOUT_SECONDS {exc}") from exc


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
        "cardano-cli not found; pass --cardano-cli or run dev/reference/setup-reference.sh"
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


def bytes_from_subprocess(value: bytes | str | None) -> bytes:
    if value is None:
        return b""
    if isinstance(value, bytes):
        return value
    return value.encode("utf-8", errors="replace")


def run_query(
    cardano_cli: str,
    socket_path: Path,
    query: str,
    net_args: list[str],
    timeout_seconds: float,
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
    try:
        proc = subprocess.run(
            command,
            capture_output=True,
            check=False,
            timeout=timeout_seconds,
        )
        stdout = proc.stdout
        stderr = proc.stderr
        exit_code: int | None = proc.returncode
        timed_out = False
    except subprocess.TimeoutExpired as exc:
        stdout = bytes_from_subprocess(exc.stdout)
        stderr = bytes_from_subprocess(exc.stderr)
        exit_code = None
        timed_out = True
    normalized, normalize_error = normalize_json(stdout)
    return {
        "command": command,
        "exit_code": exit_code,
        "timed_out": timed_out,
        "timeout_seconds": timeout_seconds,
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


def compare_results(
    ygg: dict[str, Any],
    haskell: dict[str, Any] | None,
    require_byte_equal: bool,
    require_normalized_equal: bool,
) -> tuple[str, list[str]]:
    failures: list[str] = []
    if ygg["timed_out"]:
        failures.append(
            f"yggdrasil cardano-cli query timed out after {ygg['timeout_seconds']}s"
        )
    elif ygg["exit_code"] != 0:
        failures.append("yggdrasil cardano-cli query exited non-zero")
    if not ygg["timed_out"] and ygg["normalize_error"] is not None:
        failures.append("yggdrasil stdout was not valid JSON")

    if haskell is None:
        return ("pass" if not failures else "fail"), failures

    if haskell["timed_out"]:
        failures.append(
            f"haskell cardano-cli query timed out after {haskell['timeout_seconds']}s"
        )
    elif haskell["exit_code"] != 0:
        failures.append("haskell cardano-cli query exited non-zero")
    if not haskell["timed_out"] and haskell["normalize_error"] is not None:
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


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True), encoding="utf-8")


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
    if args.timeout_seconds <= 0:
        fail("--timeout-seconds must be positive")
    equality_required = args.require_byte_equal or args.require_normalized_equal
    if not args.self_test and args.require_haskell and not equality_required:
        fail("--require-haskell requires --require-byte-equal or --require-normalized-equal")
    if not args.self_test and equality_required and not args.require_haskell:
        fail("--require-byte-equal/--require-normalized-equal require --require-haskell")
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
    if (
        not args.self_test
        and args.write_fixture is not None
        and (not args.require_haskell or not equality_required)
    ):
        fail(
            "--write-fixture requires --require-haskell plus "
            "--require-byte-equal or --require-normalized-equal"
        )
    if args.self_test:
        return
    validate_socket(args.ygg_socket, "--ygg-socket", fail)
    if args.haskell_socket is not None:
        validate_socket(args.haskell_socket, "--haskell-socket", fail)


def validate_socket(
    path: Path,
    flag: str,
    fail: Callable[[str], None],
) -> None:
    if not path.exists():
        fail(f"{flag} path does not exist: {path}")
    if not path.is_socket():
        fail(f"{flag} path is not a Unix domain socket: {path}")


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
        "--timeout-seconds",
        type=parse_timeout_seconds,
        default=default_timeout_seconds(),
        help=(
            "Maximum seconds to wait for each cardano-cli query "
            f"(default: {DEFAULT_TIMEOUT_SECONDS:g})"
        ),
    )
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
    parser.add_argument(
        "--write-fixture",
        type=Path,
        help=(
            "Write a normalized R178 fixture after strict Yggdrasil/Haskell "
            "comparison passes. Requires --require-haskell and an equality flag."
        ),
    )
    args = parser.parse_args()
    validate_required_args(args, parser)
    return args


def sample_result(
    *,
    exit_code: int = 0,
    stdout: bytes = b'{"slot":2,"hash":"abc"}\n',
    stderr: bytes = b"",
    timed_out: bool = False,
) -> dict[str, Any]:
    normalized, normalize_error = normalize_json(stdout)
    return {
        "command": ["cardano-cli", "conway", "query", "gov-state"],
        "exit_code": None if timed_out else exit_code,
        "timed_out": timed_out,
        "timeout_seconds": DEFAULT_TIMEOUT_SECONDS,
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


def fixture_result(result: dict[str, Any]) -> dict[str, Any]:
    return {
        "exit_code": result["exit_code"],
        "timed_out": result["timed_out"],
        "timeout_seconds": result["timeout_seconds"],
        "stdout_sha256": result["stdout_sha256"],
        "stderr_sha256": result["stderr_sha256"],
        "stdout_len": result["stdout_len"],
        "stderr_len": result["stderr_len"],
        "normalized_json": result["normalized_json"],
        "normalized_json_sha256": result["normalized_json_sha256"],
        "normalize_error": result["normalize_error"],
    }


def fixture_cli_version(summary: dict[str, Any]) -> dict[str, Any]:
    version = summary["cardano_cli_version"]
    return {
        "exit_code": version["exit_code"],
        "stdout": version["stdout"],
        "stderr": version["stderr"],
        "stdout_sha256": version["stdout_sha256"],
        "stderr_sha256": version["stderr_sha256"],
    }


def build_fixture(summary: dict[str, Any]) -> dict[str, Any]:
    if summary.get("status") != "pass":
        raise SystemExit("cannot write R178 fixture unless aggregate status is pass")
    equality_required = summary.get("require_byte_equal") or summary.get(
        "require_normalized_equal"
    )
    if not summary.get("require_haskell") or not equality_required:
        raise SystemExit(
            "cannot write R178 fixture without strict Haskell equality mode"
        )

    queries: dict[str, Any] = {}
    for query, result in summary["queries"].items():
        haskell = result.get("haskell")
        yggdrasil = result.get("yggdrasil")
        if result.get("status") != "pass" or yggdrasil is None or haskell is None:
            raise SystemExit(f"cannot write R178 fixture for non-passing query {query}")
        queries[query] = {
            "status": result["status"],
            "raw_stdout_comparison": result["raw_stdout_comparison"],
            "raw_stderr_comparison": result["raw_stderr_comparison"],
            "normalized_json": yggdrasil["normalized_json"],
            "yggdrasil": fixture_result(yggdrasil),
            "haskell": fixture_result(haskell),
        }

    return {
        "schema_version": 1,
        "blocker": "r178-conway-lsq",
        "generated_at_utc": summary["generated_at_utc"],
        "closeout_mode": {
            "require_haskell": summary["require_haskell"],
            "require_byte_equal": summary["require_byte_equal"],
            "require_normalized_equal": summary["require_normalized_equal"],
        },
        "status": summary["status"],
        "network_args": summary["network_args"],
        "require_byte_equal": summary["require_byte_equal"],
        "require_normalized_equal": summary["require_normalized_equal"],
        "cardano_cli_version": fixture_cli_version(summary),
        "queries": queries,
    }


def run_self_test() -> int:
    assert network_args("mainnet", None) == ["--mainnet"]
    assert network_args("preprod", None) == ["--testnet-magic", "1"]
    assert network_args("preview", 42) == ["--testnet-magic", "42"]
    assert parse_timeout_seconds("1.5") == 1.5
    for raw_timeout in ("0", "-1", "not-a-number"):
        try:
            parse_timeout_seconds(raw_timeout)
        except argparse.ArgumentTypeError as exc:
            assert "positive number" in str(exc)
        else:
            raise AssertionError(f"expected timeout {raw_timeout!r} to be rejected")

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

    timed_out = sample_result(timed_out=True, stdout=b"", stderr=b"")
    status, failures = compare_results(timed_out, None, False, False)
    assert status == "fail"
    assert "timed out" in failures[0]

    try:
        validate_required_args(
            argparse.Namespace(
                self_test=False,
                ygg_socket=Path("node.sock"),
                haskell_socket=None,
                require_haskell=True,
                require_byte_equal=True,
                require_normalized_equal=False,
                timeout_seconds=DEFAULT_TIMEOUT_SECONDS,
                write_fixture=None,
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
                haskell_socket=Path("haskell.sock"),
                require_haskell=False,
                require_byte_equal=True,
                require_normalized_equal=False,
                timeout_seconds=DEFAULT_TIMEOUT_SECONDS,
                write_fixture=None,
            )
        )
    except SystemExit as exc:
        assert "require --require-haskell" in str(exc)
    else:
        raise AssertionError("expected equality mode to reject missing --require-haskell")

    try:
        validate_required_args(
            argparse.Namespace(
                self_test=False,
                ygg_socket=Path("node.sock"),
                haskell_socket=Path("haskell.sock"),
                require_haskell=True,
                require_byte_equal=False,
                require_normalized_equal=False,
                timeout_seconds=DEFAULT_TIMEOUT_SECONDS,
                write_fixture=None,
            )
        )
    except SystemExit as exc:
        assert "--require-haskell requires" in str(exc)
    else:
        raise AssertionError("expected require_haskell to require equality mode")

    try:
        validate_required_args(
            argparse.Namespace(
                self_test=False,
                ygg_socket=Path("node.sock"),
                haskell_socket=None,
                require_haskell=False,
                require_byte_equal=False,
                require_normalized_equal=False,
                timeout_seconds=0,
                write_fixture=None,
            )
        )
    except SystemExit as exc:
        assert "--timeout-seconds must be positive" in str(exc)
    else:
        raise AssertionError("expected non-positive timeout to be rejected")

    with tempfile.TemporaryDirectory() as tmp:
        missing_socket = Path(tmp) / "missing.socket"
        regular_file = Path(tmp) / "regular.socket"
        regular_file.write_text("not a socket", encoding="utf-8")
        for candidate, message in (
            (missing_socket, "path does not exist"),
            (regular_file, "not a Unix domain socket"),
        ):
            try:
                validate_required_args(
                    argparse.Namespace(
                        self_test=False,
                        ygg_socket=candidate,
                        haskell_socket=None,
                        require_haskell=False,
                        require_byte_equal=False,
                        require_normalized_equal=False,
                        timeout_seconds=DEFAULT_TIMEOUT_SECONDS,
                        write_fixture=None,
                    )
                )
            except SystemExit as exc:
                assert message in str(exc)
            else:
                raise AssertionError(f"expected {candidate} to be rejected")

    try:
        validate_required_args(
            argparse.Namespace(
                self_test=False,
                ygg_socket=Path("node.sock"),
                haskell_socket=Path("haskell.sock"),
                require_haskell=False,
                require_byte_equal=False,
                require_normalized_equal=False,
                timeout_seconds=DEFAULT_TIMEOUT_SECONDS,
                write_fixture=Path("r178-fixture.json"),
            )
        )
    except SystemExit as exc:
        assert "--write-fixture requires" in str(exc)
    else:
        raise AssertionError("expected write_fixture to require strict closeout mode")

    fixture_query = {
        "status": "pass",
        "failures": [],
        "raw_stdout_comparison": compare_raw_bytes(
            ygg["_stdout_bytes"],
            haskell_same_json["_stdout_bytes"],
        ),
        "raw_stderr_comparison": compare_raw_bytes(
            ygg["_stderr_bytes"],
            haskell_same_json["_stderr_bytes"],
        ),
        "yggdrasil": json_ready_result(ygg),
        "haskell": json_ready_result(haskell_same_json),
    }
    fixture_summary = {
        "generated_at_utc": dt.datetime.now(dt.UTC).isoformat(),
        "status": "pass",
        "cardano_cli_version": {
            "command": ["cardano-cli", "--version"],
            "exit_code": 0,
            "stdout": "cardano-cli 11.0.0.0\n",
            "stderr": "",
            "stdout_sha256": sha256_bytes(b"cardano-cli 11.0.0.0\n"),
            "stderr_sha256": sha256_bytes(b""),
        },
        "network_args": ["--testnet-magic", "2"],
        "require_haskell": True,
        "require_byte_equal": False,
        "require_normalized_equal": True,
        "queries": {query: fixture_query for query in DEFAULT_QUERIES},
    }
    fixture = build_fixture(fixture_summary)
    assert fixture["schema_version"] == 1
    assert fixture["blocker"] == "r178-conway-lsq"
    assert fixture["generated_at_utc"]
    assert fixture["closeout_mode"] == {
        "require_haskell": True,
        "require_byte_equal": False,
        "require_normalized_equal": True,
    }
    assert sorted(fixture["queries"]) == sorted(DEFAULT_QUERIES)
    assert fixture["queries"]["gov-state"]["normalized_json"] == (
        '{"hash":"abc","slot":2}'
    )
    assert "command" not in fixture["queries"]["gov-state"]["yggdrasil"]
    assert not fixture["queries"]["gov-state"]["raw_stdout_comparison"]["byte_equal"]
    with tempfile.TemporaryDirectory() as tmp:
        fixture_path = Path(tmp) / "r178-fixture.json"
        write_json(fixture_path, fixture)
        reloaded = json.loads(fixture_path.read_text(encoding="utf-8"))
        assert reloaded["queries"]["gov-state"]["haskell"]["stdout_sha256"] == (
            haskell_same_json["stdout_sha256"]
        )
    write_json(SELF_TEST_FIXTURE, fixture)

    fixture_summary["status"] = "fail"
    try:
        build_fixture(fixture_summary)
    except SystemExit as exc:
        assert "aggregate status is pass" in str(exc)
    else:
        raise AssertionError("expected non-passing fixture summary to be rejected")

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
        "generated_at_utc": dt.datetime.now(dt.UTC).isoformat(),
        "cardano_cli": cardano_cli,
        "cardano_cli_version": cli_version,
        "network_args": net_args,
        "ygg_socket": str(args.ygg_socket),
        "haskell_socket": str(args.haskell_socket) if args.haskell_socket else None,
        "require_haskell": args.require_haskell,
        "require_byte_equal": args.require_byte_equal,
        "require_normalized_equal": args.require_normalized_equal,
        "write_fixture": str(args.write_fixture) if args.write_fixture else None,
        "timeout_seconds": args.timeout_seconds,
        "queries": {},
    }

    failed = False
    for query in queries:
        ygg = run_query(
            cardano_cli,
            args.ygg_socket,
            query,
            net_args,
            args.timeout_seconds,
        )
        haskell = (
            run_query(
                cardano_cli,
                args.haskell_socket,
                query,
                net_args,
                args.timeout_seconds,
            )
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

    summary["status"] = "fail" if failed else "pass"
    summary_path = artifact_dir / "summary.json"
    write_json(summary_path, summary)
    if args.write_fixture is not None and summary["status"] == "pass":
        write_json(args.write_fixture, build_fixture(summary))
    print(f"wrote {summary_path}")
    if args.write_fixture is not None and summary["status"] == "pass":
        print(f"wrote fixture {args.write_fixture}")
    for query, result in summary["queries"].items():
        print(f"{query}: {result['status']}")
        for failure in result["failures"]:
            print(f"  - {failure}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
