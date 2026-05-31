#!/usr/bin/env python3
"""Compare Gap BP V2 ScriptContext CBOR dumps.

This is an offline evidence harness for the remaining Gap BP Plutus V2 drift.
It compares the Rust `YGG_DUMP_SCRIPT_CONTEXT` capture against a future
upstream Haskell capture for the same preview transaction.  The script accepts
either full log lines containing `cbor_hex=` or files containing raw hex bytes,
writes replayable CBOR artifacts, and reports the first divergent byte window.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_RUST_LOG = (
    ROOT
    / "docs"
    / "operational-runs"
    / "2026-05-06-round-266c-gap-bp-script-context.log"
)
HEX_RE = re.compile(r"^[0-9a-fA-F\s]+$")


@dataclass(frozen=True)
class Dump:
    label: str
    source: str
    cbor: bytes
    metadata: dict[str, str]

    @property
    def sha256(self) -> str:
        return hashlib.sha256(self.cbor).hexdigest()

    def summary(self) -> dict[str, Any]:
        return {
            "label": self.label,
            "source": self.source,
            "metadata": self.metadata,
            "cbor_len": len(self.cbor),
            "cbor_sha256": self.sha256,
        }


def parse_key_values(line: str) -> dict[str, str]:
    metadata: dict[str, str] = {}
    for token in line.strip().split():
        if "=" not in token:
            continue
        key, value = token.split("=", 1)
        if key.endswith(":"):
            key = key[:-1]
        metadata[key] = value
    return metadata


def extract_hex_from_text(text: str) -> tuple[str, dict[str, str]]:
    for line in text.splitlines():
        if "cbor_hex=" not in line:
            continue
        metadata = parse_key_values(line)
        if "cbor_hex" not in metadata:
            raise SystemExit("line contains cbor_hex= but could not parse cbor_hex token")
        return metadata["cbor_hex"], metadata

    stripped = "".join(text.split())
    if stripped and HEX_RE.fullmatch(text):
        return stripped, {}
    raise SystemExit("input must contain a cbor_hex= log line or raw hexadecimal bytes")


def decode_hex(hex_text: str, label: str) -> bytes:
    if len(hex_text) % 2 != 0:
        raise SystemExit(f"{label}: hex length must be even")
    try:
        return bytes.fromhex(hex_text)
    except ValueError as exc:
        raise SystemExit(f"{label}: invalid hex: {exc}") from exc


def load_dump(label: str, path: Path) -> Dump:
    text = path.read_text(encoding="utf-8")
    hex_text, metadata = extract_hex_from_text(text)
    cbor = decode_hex(hex_text, label)
    declared_len = metadata.get("cbor_len")
    if declared_len is not None and int(declared_len) != len(cbor):
        raise SystemExit(
            f"{label}: cbor_len={declared_len} but decoded {len(cbor)} bytes"
        )
    return Dump(label=label, source=str(path), cbor=cbor, metadata=metadata)


def first_diff(left: bytes, right: bytes) -> int | None:
    for index, (left_byte, right_byte) in enumerate(zip(left, right)):
        if left_byte != right_byte:
            return index
    if len(left) != len(right):
        return min(len(left), len(right))
    return None


def diff_window(left: bytes, right: bytes, offset: int, radius: int = 24) -> dict[str, Any]:
    start = max(0, offset - radius)
    end = min(max(len(left), len(right)), offset + radius)
    return {
        "offset": offset,
        "start": start,
        "end": end,
        "rust_hex": left[start:min(end, len(left))].hex(),
        "haskell_hex": right[start:min(end, len(right))].hex(),
    }


def write_artifacts(artifact_dir: Path, rust: Dump, haskell: Dump | None) -> None:
    artifact_dir.mkdir(parents=True, exist_ok=True)
    (artifact_dir / "rust-script-context.cbor").write_bytes(rust.cbor)
    (artifact_dir / "rust-script-context.hex").write_text(
        rust.cbor.hex() + "\n", encoding="utf-8"
    )
    if haskell is not None:
        (artifact_dir / "haskell-script-context.cbor").write_bytes(haskell.cbor)
        (artifact_dir / "haskell-script-context.hex").write_text(
            haskell.cbor.hex() + "\n", encoding="utf-8"
        )


def compare(rust: Dump, haskell: Dump | None) -> dict[str, Any]:
    result: dict[str, Any] = {
        "rust": rust.summary(),
        "haskell": haskell.summary() if haskell is not None else None,
        "comparison": None,
    }
    if haskell is None:
        return result

    offset = first_diff(rust.cbor, haskell.cbor)
    byte_equal = offset is None
    result["comparison"] = {
        "byte_equal": byte_equal,
        "first_diff_offset": offset,
        "length_delta": len(rust.cbor) - len(haskell.cbor),
        "diff_window": None if byte_equal else diff_window(rust.cbor, haskell.cbor, offset),
    }
    return result


def validate_required_args(
    args: argparse.Namespace,
    parser: argparse.ArgumentParser | None = None,
) -> None:
    if not args.self_test and args.require_byte_equal and args.haskell_log is None:
        if parser is not None:
            parser.error("--haskell-log is required with --require-byte-equal")
        raise SystemExit("--haskell-log is required with --require-byte-equal")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Compare Rust and Haskell Gap BP ScriptContext CBOR dumps"
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="Run local parser/comparison checks without the captured fixture",
    )
    parser.add_argument(
        "--rust-log",
        type=Path,
        default=DEFAULT_RUST_LOG,
        help="Rust YGG_DUMP_SCRIPT_CONTEXT log or raw hex file",
    )
    parser.add_argument(
        "--haskell-log",
        type=Path,
        help="Haskell ScriptContext log or raw hex file for the same tx",
    )
    parser.add_argument(
        "--artifact-dir",
        type=Path,
        default=Path("target/gap-bp-script-context-comparison"),
    )
    parser.add_argument(
        "--require-byte-equal",
        action="store_true",
        help="Require --haskell-log and exit non-zero when CBOR bytes differ",
    )
    args = parser.parse_args()
    validate_required_args(args, parser)
    return args


def expect_system_exit(func: Any, expected_fragment: str) -> None:
    try:
        func()
    except SystemExit as exc:
        if expected_fragment not in str(exc):
            raise AssertionError(
                f"expected SystemExit containing {expected_fragment!r}, got {exc!r}"
            ) from exc
        return
    raise AssertionError(f"expected SystemExit containing {expected_fragment!r}")


def run_self_test() -> int:
    raw_hex, raw_metadata = extract_hex_from_text("d8799fff\n")
    assert raw_hex == "d8799fff"
    assert raw_metadata == {}

    log_hex, log_metadata = extract_hex_from_text(
        "YGG_DUMP_SCRIPT_CONTEXT: tx_hash=aa script_hash=bb "
        "version=V2 cbor_len=4 cbor_hex=d8799fff"
    )
    assert log_hex == "d8799fff"
    assert log_metadata["tx_hash"] == "aa"
    assert log_metadata["version"] == "V2"
    assert decode_hex(log_hex, "self") == bytes.fromhex("d8799fff")

    expect_system_exit(lambda: decode_hex("abc", "odd"), "hex length must be even")
    expect_system_exit(
        lambda: extract_hex_from_text("not a cbor dump"), "input must contain"
    )

    rust = Dump("rust", "self", bytes.fromhex("d8799fff"), log_metadata)
    same = Dump("haskell", "self", bytes.fromhex("d8799fff"), log_metadata)
    different = Dump("haskell", "self", bytes.fromhex("d8799f00"), log_metadata)
    assert compare(rust, None)["comparison"] is None
    assert compare(rust, same)["comparison"]["byte_equal"] is True
    mismatch = compare(rust, different)["comparison"]
    assert mismatch["byte_equal"] is False
    assert mismatch["first_diff_offset"] == 3
    assert mismatch["diff_window"]["rust_hex"].endswith("ff")
    assert mismatch["diff_window"]["haskell_hex"].endswith("00")
    expect_system_exit(
        lambda: validate_required_args(
            argparse.Namespace(
                self_test=False,
                require_byte_equal=True,
                haskell_log=None,
            )
        ),
        "--haskell-log is required",
    )

    with tempfile.TemporaryDirectory(prefix="gap-bp-script-context-self-") as tmp:
        tmp_path = Path(tmp)
        rust_path = tmp_path / "rust.log"
        bad_len_path = tmp_path / "bad.log"
        artifact_dir = tmp_path / "artifacts"
        rust_path.write_text(
            "YGG_DUMP_SCRIPT_CONTEXT: tx_hash=aa script_hash=bb "
            "version=V2 cbor_len=4 cbor_hex=d8799fff\n",
            encoding="utf-8",
        )
        bad_len_path.write_text(
            "YGG_DUMP_SCRIPT_CONTEXT: tx_hash=aa script_hash=bb "
            "version=V2 cbor_len=5 cbor_hex=d8799fff\n",
            encoding="utf-8",
        )
        loaded = load_dump("rust", rust_path)
        assert loaded.cbor == rust.cbor
        expect_system_exit(
            lambda: load_dump("bad", bad_len_path), "but decoded 4 bytes"
        )
        write_artifacts(artifact_dir, rust, same)
        assert (artifact_dir / "rust-script-context.cbor").read_bytes() == rust.cbor
        assert (artifact_dir / "haskell-script-context.hex").read_text(
            encoding="utf-8"
        ).strip() == same.cbor.hex()

    print("[ok] compare-gap-bp-script-context self-test passed")
    return 0


def main() -> int:
    args = parse_args()
    if args.self_test:
        return run_self_test()

    rust = load_dump("rust", args.rust_log)
    haskell = load_dump("haskell", args.haskell_log) if args.haskell_log else None
    write_artifacts(args.artifact_dir, rust, haskell)
    summary = compare(rust, haskell)

    summary_path = args.artifact_dir / "summary.json"
    summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True), encoding="utf-8")
    print(f"wrote {summary_path}")
    print(f"rust: len={len(rust.cbor)} sha256={rust.sha256}")
    if haskell is None:
        print("haskell: not provided")
        return 0

    comparison = summary["comparison"]
    print(f"haskell: len={len(haskell.cbor)} sha256={haskell.sha256}")
    print(f"byte_equal={comparison['byte_equal']}")
    if not comparison["byte_equal"]:
        print(f"first_diff_offset={comparison['first_diff_offset']}")
    if args.require_byte_equal and not comparison["byte_equal"]:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
