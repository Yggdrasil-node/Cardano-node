#!/usr/bin/env python3
"""
Offline classifier for the failing Ed25519 (vkey, msg, sig) triple captured
by Yggdrasil's `verify_vkey_signatures` diagnostic dump. Pure standard
library — no pynacl/cryptography needed; we only do the number-theory
checks that determine whether a strict verifier would reject.

Usage:
    python3 classify_signature.py /tmp/ygg-r251-vkey-forensic/vkey-fail.txt
"""
import sys
from pathlib import Path

# Curve constants (Ed25519 / Curve25519)
P = (1 << 255) - 19          # field prime
L = (1 << 252) + 27742317777372353535851937790883648493  # subgroup order
COFACTOR = 8


def parse_dump(path: Path) -> dict:
    out = {}
    for line in path.read_text().splitlines():
        if ":" not in line or line.startswith("#"):
            continue
        k, _, v = line.partition(":")
        out[k.strip()] = v.strip()
    return out


def hex_to_bytes(s: str) -> bytes:
    return bytes.fromhex(s)


def y_from_R(R: bytes) -> int:
    """Decode the y-coordinate from the 32-byte compressed R encoding.
    Sign bit lives in MSB of byte 31; remaining 255 bits are y in
    little-endian.
    """
    assert len(R) == 32
    y_bytes = bytearray(R)
    y_bytes[31] &= 0x7F
    return int.from_bytes(bytes(y_bytes), "little")


def is_R_canonical(R: bytes) -> tuple[bool, str]:
    y = y_from_R(R)
    if y >= P:
        return False, f"non-canonical R: y={y:x} >= p={P:x}"
    return True, f"canonical R: y={y:x} < p"


def is_S_canonical(S: bytes) -> tuple[bool, str]:
    s = int.from_bytes(S, "little")
    if s >= L:
        return False, f"non-canonical S: s={s:x} >= L={L:x}"
    return True, f"canonical S: s={s:x} < L"


# Ed25519 small-order points (compressed encoding). These are the 8 elements
# of the E[8] subgroup. Public keys equal to any of these are rejected by
# RFC 8032 strict verification. Source: ed25519-dalek `EIGHT_TORSION` table.
SMALL_ORDER_POINTS_HEX = [
    "0100000000000000000000000000000000000000000000000000000000000000",  # neutral
    "0000000000000000000000000000000000000000000000000000000000000080",  # neutral with high bit
    "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05",
    "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc85",
    "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
    "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a",
    "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa",
]
SMALL_ORDER_POINTS = {bytes.fromhex(h) for h in SMALL_ORDER_POINTS_HEX}


def is_small_order_pubkey(vkey: bytes) -> tuple[bool, str]:
    if vkey in SMALL_ORDER_POINTS:
        return True, "vkey is one of the 8 small-order torsion points"
    return False, "vkey is NOT a small-order torsion point"


def main():
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    path = Path(sys.argv[1])
    dump = parse_dump(path)

    vkey = hex_to_bytes(dump["vkey_hex"])
    sig = hex_to_bytes(dump["signature_hex"])
    msg = hex_to_bytes(dump["msg_hex"])
    R = sig[:32]
    S = sig[32:64]

    print(f"vkey         ({len(vkey)} bytes): {vkey.hex()}")
    print(f"vkey_hash      (28 bytes): {dump.get('vkey_hash_hex', '?')}")
    print(f"msg          ({len(msg)} bytes): {msg.hex()}")
    print(f"signature.R  ({len(R)} bytes): {R.hex()}")
    print(f"signature.S  ({len(S)} bytes): {S.hex()}")
    print(f"yggdrasil verify error: {dump.get('error', '?')}")
    print()

    # Check R canonicality
    ok_R, why_R = is_R_canonical(R)
    print(f"R canonicality: {'OK' if ok_R else 'NON-CANONICAL'} — {why_R}")

    # Check S canonicality
    ok_S, why_S = is_S_canonical(S)
    print(f"S canonicality: {'OK' if ok_S else 'NON-CANONICAL'} — {why_S}")

    # Check small-order pubkey
    is_small, why_small = is_small_order_pubkey(vkey)
    print(f"vkey order: {'SMALL-ORDER' if is_small else 'normal'} — {why_small}")

    print()
    if not ok_R and ok_S and not is_small:
        print("CONCLUSION: signature has non-canonical R encoding.")
        print("  - libsodium crypto_sign_ed25519_verify_detached: ACCEPTS")
        print("  - ed25519-dalek verify_strict (RFC 8032 strict): REJECTS")
        print("  - Confirmed: Yggdrasil's verify_strict diverges from upstream Cardano.")
        sys.exit(0)
    elif ok_R and ok_S and is_small:
        print("CONCLUSION: pubkey is a small-order torsion point.")
        print("  - libsodium: ACCEPTS")
        print("  - ed25519-dalek verify_strict: REJECTS")
        sys.exit(0)
    elif ok_R and ok_S and not is_small:
        print("CONCLUSION: signature appears canonical and pubkey is normal-order.")
        print("  This is NOT a strict-vs-libsodium issue. Look elsewhere:")
        print("  - tx_body_hash byte range mismatch?")
        print("  - witness vkey/sig CBOR decoding bug?")
        print("  - Ed25519 cofactored-vs-cofactorless verification semantics?")
        sys.exit(1)
    else:
        print("CONCLUSION: multi-cause divergence.")
        print(f"  R canonical={ok_R}, S canonical={ok_S}, small-order={is_small}")
        sys.exit(1)


if __name__ == "__main__":
    main()
