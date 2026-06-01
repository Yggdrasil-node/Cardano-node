#!/usr/bin/env python3
"""Compare Gap BO TPraos VRF evidence lines.

This is an offline evidence helper for the preprod Gap BO TPraos replay.  Rust
emits `TPRAOS_VRF_EVIDENCE` lines when `YGG_DUMP_TPRAOS_VRF=1`; the matching
Haskell/operator capture can be transformed into the same key-value shape, then
this script compares overlay classification, active delegate selection, nonce
state, VRF seeds, VRF outputs, and proof hashes by slot.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any


EVIDENCE_PREFIX = "TPRAOS_VRF_EVIDENCE"
KEY_RE = re.compile(r"([A-Za-z_][A-Za-z0-9_]*)=")
DEFAULT_GAP_BO_TARGET_SLOT = 429460
REQUIRED_METADATA_KEYS = ("slot", "era", "verification")
DEFAULT_COMPARE_KEYS = (
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


@dataclass(frozen=True)
class Evidence:
    label: str
    source: str
    line_number: int
    fields: dict[str, str]

    @property
    def slot(self) -> int:
        try:
            return int(self.fields["slot"])
        except (KeyError, ValueError) as exc:
            raise SystemExit(
                f"{self.label}:{self.source}:{self.line_number}: missing or invalid slot"
            ) from exc

    def summary(self) -> dict[str, Any]:
        return {
            "label": self.label,
            "source": self.source,
            "line_number": self.line_number,
            "slot": self.slot,
            "fields": self.fields,
        }


def parse_evidence_line(line: str) -> dict[str, str] | None:
    if EVIDENCE_PREFIX not in line:
        return None
    start = line.index(EVIDENCE_PREFIX) + len(EVIDENCE_PREFIX)
    body = line[start:].strip()
    matches = list(KEY_RE.finditer(body))
    if not matches:
        raise SystemExit(f"evidence line has no key=value fields: {line!r}")

    fields: dict[str, str] = {}
    for index, match in enumerate(matches):
        key = match.group(1)
        value_start = match.end()
        value_end = matches[index + 1].start() if index + 1 < len(matches) else len(body)
        fields[key] = body[value_start:value_end].strip()
    return fields


def load_evidence(label: str, path: Path, slot: int | None) -> list[Evidence]:
    entries: list[Evidence] = []
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        fields = parse_evidence_line(line)
        if fields is None:
            continue
        evidence = Evidence(label, str(path), line_number, fields)
        if slot is None or evidence.slot == slot:
            entries.append(evidence)
    if not entries:
        scope = f" for slot {slot}" if slot is not None else ""
        raise SystemExit(f"{label}: no {EVIDENCE_PREFIX} lines found in {path}{scope}")
    return entries


def index_by_slot(entries: list[Evidence]) -> dict[int, Evidence]:
    indexed: dict[int, Evidence] = {}
    for evidence in entries:
        if evidence.slot in indexed:
            raise SystemExit(
                f"{evidence.label}: duplicate evidence for slot {evidence.slot}; pass --slot"
            )
        indexed[evidence.slot] = evidence
    return indexed


def required_evidence_keys(compare_keys: tuple[str, ...]) -> tuple[str, ...]:
    return tuple(dict.fromkeys((*REQUIRED_METADATA_KEYS, *compare_keys)))


def validate_required_keys(entries: list[Evidence], keys: tuple[str, ...]) -> None:
    required = required_evidence_keys(keys)
    for evidence in entries:
        missing = [key for key in required if key not in evidence.fields]
        if missing:
            joined = ", ".join(missing)
            raise SystemExit(
                f"{evidence.label}:{evidence.source}:{evidence.line_number}: "
                f"missing required evidence keys: {joined}"
            )


def compare_evidence(
    rust_entries: list[Evidence],
    haskell_entries: list[Evidence] | None,
    keys: tuple[str, ...],
) -> tuple[list[dict[str, Any]], bool]:
    if haskell_entries is None:
        return (
            [
                {
                    "slot": evidence.slot,
                    "status": "captured",
                    "rust": evidence.summary(),
                    "haskell": None,
                    "mismatches": [],
                }
                for evidence in rust_entries
            ],
            False,
        )

    rust_by_slot = index_by_slot(rust_entries)
    haskell_by_slot = index_by_slot(haskell_entries)
    all_slots = sorted(set(rust_by_slot) | set(haskell_by_slot))
    results: list[dict[str, Any]] = []
    failed = False
    for slot in all_slots:
        rust = rust_by_slot.get(slot)
        haskell = haskell_by_slot.get(slot)
        mismatches: list[dict[str, str]] = []
        if rust is None or haskell is None:
            failed = True
            results.append(
                {
                    "slot": slot,
                    "status": "missing",
                    "rust": rust.summary() if rust else None,
                    "haskell": haskell.summary() if haskell else None,
                    "mismatches": [],
                }
            )
            continue

        for key in keys:
            rust_value = rust.fields.get(key)
            haskell_value = haskell.fields.get(key)
            if rust_value != haskell_value:
                mismatches.append(
                    {
                        "key": key,
                        "rust": "<missing>" if rust_value is None else rust_value,
                        "haskell": "<missing>" if haskell_value is None else haskell_value,
                    }
                )
        failed = failed or bool(mismatches)
        results.append(
            {
                "slot": slot,
                "status": "fail" if mismatches else "pass",
                "rust": rust.summary(),
                "haskell": haskell.summary(),
                "mismatches": mismatches,
            }
        )
    return results, failed


def target_slot_status(results: list[dict[str, Any]], target_slot: int) -> dict[str, Any]:
    matching = [result for result in results if result["slot"] == target_slot]
    return {
        "slot": target_slot,
        "present": bool(matching),
        "status": matching[0]["status"] if matching else "missing",
    }


def validate_required_args(
    args: argparse.Namespace,
    parser: argparse.ArgumentParser | None = None,
) -> None:
    def fail(message: str) -> None:
        if parser is not None:
            parser.error(message)
        raise SystemExit(message)

    if not args.self_test and args.rust_log is None:
        fail("--rust-log is required unless --self-test is set")
    if args.slot is not None and args.slot < 0:
        fail("--slot must be non-negative")
    if args.target_slot < 0:
        fail("--target-slot must be non-negative")
    if not args.self_test and args.require_equal and args.haskell_log is None:
        fail("--haskell-log is required with --require-equal")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Compare Gap BO TPraos VRF evidence by slot"
    )
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--rust-log", type=Path)
    parser.add_argument("--haskell-log", type=Path)
    parser.add_argument("--slot", type=int)
    parser.add_argument(
        "--target-slot",
        type=int,
        default=DEFAULT_GAP_BO_TARGET_SLOT,
        help=(
            "Slot that must be present for strict Gap BO closeout "
            f"(default: {DEFAULT_GAP_BO_TARGET_SLOT})"
        ),
    )
    parser.add_argument(
        "--key",
        action="append",
        help="Evidence key to compare; repeatable. Defaults to Gap BO parity keys.",
    )
    parser.add_argument(
        "--artifact-dir",
        type=Path,
        default=Path("target/gap-bo-tpraos-vrf-comparison"),
    )
    parser.add_argument(
        "--require-equal",
        action="store_true",
        help="Require --haskell-log and exit non-zero when compared fields differ",
    )
    args = parser.parse_args()
    validate_required_args(args, parser)
    return args


def write_summary(path: Path, summary: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(summary, indent=2, sort_keys=True), encoding="utf-8")


def expect_system_exit(func: Any, expected_fragment: str) -> None:
    try:
        func()
    except SystemExit as exc:
        if expected_fragment not in str(exc):
            raise AssertionError(
                f"expected SystemExit containing {expected_fragment!r}, got {exc!r}"
            ) from exc
    else:
        raise AssertionError(f"expected SystemExit containing {expected_fragment!r}")


def run_self_test() -> int:
    line = (
        "TPRAOS_VRF_EVIDENCE slot=429460 era=Shelley verification=err:InvalidVrf "
        "classification=active first_slot=86400 d=1/1 offset=343060 position=343060 "
        "asc_inv=20 genesis_idx=0 genesis_hash=aa expected_delegate_hash=bb "
        "actual_delegate_hash=bb expected_vrf_key_hash=cc actual_vrf_key_hash=cc "
        "current_epoch=4 epoch_nonce=Hash([22, 45, 41]) evolving_nonce=Neutral "
        "candidate_nonce=Neutral prev_hash_nonce=Neutral lab_nonce=Neutral "
        "nonce_state_phase=ticked_for_verification "
        "epoch_nonce_hex=162d29c4e1cf6b8a84f2d692e67a3ac6bc7851bc3e6e4afe64d15778bed8bd86 "
        "evolving_nonce_hex=neutral candidate_nonce_hex=neutral "
        "prev_hash_nonce_hex=neutral lab_nonce_hex=neutral "
        "leader_seed=dd nonce_seed=ee leader_output=ff nonce_output=00 "
        "leader_proof_hash=11 nonce_proof_hash=22 "
        "haskell_refs=Cardano.Protocol.TPraos.Rules.Overlay.pbftVrfChecks"
    )
    parsed = parse_evidence_line(line)
    assert parsed is not None
    assert parsed["slot"] == "429460"
    assert parsed["epoch_nonce"] == "Hash([22, 45, 41])"
    assert parsed["haskell_refs"] == "Cardano.Protocol.TPraos.Rules.Overlay.pbftVrfChecks"

    rust = Evidence("rust", "self", 1, parsed)
    same = Evidence("haskell", "self", 1, dict(parsed))
    validate_required_keys([rust, same], DEFAULT_COMPARE_KEYS)
    results, failed = compare_evidence([rust], [same], DEFAULT_COMPARE_KEYS)
    assert not failed, results
    assert results[0]["status"] == "pass"

    missing = dict(parsed)
    del missing["leader_seed"]
    expect_system_exit(
        lambda: validate_required_keys(
            [Evidence("rust", "self", 2, missing)],
            DEFAULT_COMPARE_KEYS,
        ),
        "missing required evidence keys: leader_seed",
    )
    missing_metadata = dict(parsed)
    del missing_metadata["verification"]
    expect_system_exit(
        lambda: validate_required_keys(
            [Evidence("rust", "self", 3, missing_metadata)],
            DEFAULT_COMPARE_KEYS,
        ),
        "missing required evidence keys: verification",
    )
    expect_system_exit(
        lambda: validate_required_args(
            argparse.Namespace(
                self_test=False,
                rust_log=Path("rust.log"),
                haskell_log=None,
                slot=None,
                target_slot=DEFAULT_GAP_BO_TARGET_SLOT,
                require_equal=True,
            )
        ),
        "--haskell-log is required",
    )
    expect_system_exit(
        lambda: validate_required_args(
            argparse.Namespace(
                self_test=False,
                rust_log=Path("rust.log"),
                haskell_log=Path("haskell.log"),
                slot=-1,
                target_slot=DEFAULT_GAP_BO_TARGET_SLOT,
                require_equal=True,
            )
        ),
        "--slot must be non-negative",
    )
    expect_system_exit(
        lambda: validate_required_args(
            argparse.Namespace(
                self_test=False,
                rust_log=Path("rust.log"),
                haskell_log=Path("haskell.log"),
                slot=None,
                target_slot=-1,
                require_equal=True,
            )
        ),
        "--target-slot must be non-negative",
    )

    changed = dict(parsed)
    changed["leader_seed"] = "changed"
    results, failed = compare_evidence(
        [rust],
        [Evidence("haskell", "self", 2, changed)],
        DEFAULT_COMPARE_KEYS,
    )
    assert failed
    assert results[0]["mismatches"][0]["key"] == "leader_seed"
    status = target_slot_status(results, DEFAULT_GAP_BO_TARGET_SLOT)
    assert status == {
        "slot": DEFAULT_GAP_BO_TARGET_SLOT,
        "present": True,
        "status": "fail",
    }
    status = target_slot_status(results, DEFAULT_GAP_BO_TARGET_SLOT + 1)
    assert status == {
        "slot": DEFAULT_GAP_BO_TARGET_SLOT + 1,
        "present": False,
        "status": "missing",
    }

    print("[ok] compare-gap-bo-tpraos-vrf self-test passed")
    return 0


def main() -> int:
    args = parse_args()
    if args.self_test:
        return run_self_test()

    keys = tuple(args.key or DEFAULT_COMPARE_KEYS)
    rust_entries = load_evidence("rust", args.rust_log, args.slot)
    haskell_entries = (
        load_evidence("haskell", args.haskell_log, args.slot) if args.haskell_log else None
    )
    validate_required_keys(rust_entries, keys)
    if haskell_entries is not None:
        validate_required_keys(haskell_entries, keys)
    results, failed = compare_evidence(rust_entries, haskell_entries, keys)
    target_slot = target_slot_status(results, args.target_slot)
    if args.require_equal and not target_slot["present"]:
        failed = True
    summary = {
        "compare_keys": keys,
        "required_keys": required_evidence_keys(keys),
        "rust_log": str(args.rust_log),
        "haskell_log": str(args.haskell_log) if args.haskell_log else None,
        "slot": args.slot,
        "target_slot": target_slot,
        "results": results,
    }
    summary_path = args.artifact_dir / "summary.json"
    write_summary(summary_path, summary)
    print(f"wrote {summary_path}")
    for result in results:
        print(f"slot {result['slot']}: {result['status']}")
        for mismatch in result["mismatches"][:8]:
            print(
                f"  - {mismatch['key']}: rust={mismatch['rust']} "
                f"haskell={mismatch['haskell']}"
            )
    if args.require_equal and not target_slot["present"]:
        print(f"target slot {args.target_slot}: missing")
    return 1 if failed and args.require_equal else 0


if __name__ == "__main__":
    sys.exit(main())
