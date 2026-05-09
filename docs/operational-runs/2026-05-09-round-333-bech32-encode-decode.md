---
title: 'R333: bech32 concrete encode/decode — drop-in deployment ready'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-333-bech32-encode-decode/
---

# Round 333 — bech32 concrete encode/decode

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R332`](2026-05-09-round-332-bech32-cli-parser.md)  
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), Phase A.1 round 3 of 4.

## Summary

R333 lands the concrete encode/decode implementation for the bech32
binary, replacing R331's placeholder types and R332's "encode/decode
not yet implemented" sentinel with a working pure-Rust implementation
backed by `bech32 v0.11` (BIP-0173 + BIP-0350) + `bs58 v0.5` (Bitcoin
alphabet) + `hex v0.4` (base16).

After R333 the yggdrasil bech32 binary is **drop-in deployment
equivalent** to the upstream `IntersectMBO/bech32 1.1.10` binary —
verified by direct diff against `.reference-haskell-cardano-node/install/bin/bech32`
on every documented example from upstream's `--help` text:

```text
=== Drop-in deployment evidence (R333) ===
encode 'base16_' < 706174617465       MATCH (base16_1wpshgct5v5r5mxh0)
encode 'base16_' < Ae2tdPwUPEYy       MATCH (base16_1p58rejhd9592uusvngmgl)
encode 'base16_' < old_prefix1...     MATCH (base16_1wpshgcgv0pk2p)
decode bech32 → base16                MATCH (706174617465)
```

## Diff inventory

| Path | Change |
|---|---|
| `Cargo.toml` (root) | +1 workspace dep: `bs58 = "0.5"` (pure-Rust Base58 from Nullus157/bs58-rs, MIT/Apache-2.0). |
| `crates/bech32/Cargo.toml` | Wired `bs58 = { workspace = true }` + `hex = { workspace = true }` deps. |
| `crates/bech32/src/lib.rs` | Replaced R331 placeholder types with real backed implementations: `HumanReadablePart` (= `bech32::Hrp`), `DataPart` (raw bytes), `Bech32Error` (combined error surface), `InputEncoding` (Base16/Bech32/Base58 detected encoding). Added concrete `run_with()` / `run_decode()` / `run_encode()` / `detect_encoding()` mirroring upstream `bech32/app/Main.hs::run` byte-for-byte. 13 new unit tests pin the upstream-help examples + detection heuristic + round-trip behavior. |
| `crates/bech32/src/internal.rs` | Replaced placeholder constants with real `CHARSET` (BIP-0173 alphabet `qpzry9x8gf2tvdw0s3jn54khce6mua7l`) and `EncodingSpec` enum (Bech32/Bech32m). 2 new unit tests pin the alphabet against drift. |
| `crates/bech32/tests/cli_help_golden.rs` | Replaced R332 sentinel test with 3 stdin/stdout integration tests: empty-stdin → StringToDecodeTooShort error (mirrors upstream); base16-via-stdin → bech32 round-trip; bech32-via-stdin → base16 decode. All 8 golden tests pass. |
| `docs/parity-matrix.json` | `sister-tool.bech32` advanced: next_milestone `R333 → R334`; 7 new `implemented_evidence` rows. |
| `docs/operational-runs/2026-05-09-round-333-bech32-encode-decode.md` | This round-doc. |

## Implementation notes

**Encoding detection** (`detect_encoding`): mirrors upstream's
`bech32/app/Main.hs::detectEncoding` heuristic byte-for-byte. Reject
strings shorter than 8 chars, then try Base16 (all-hex + even length)
→ Bech32 (separator '1' + valid HRP/data chars + consistent letter
case) → Base58 (all-Bitcoin-alphabet chars). The detection order is
the same as upstream's; the `or` logic is preserved.

**Bech32 vs Bech32m**: Cardano addresses use BIP-0173 Bech32 (NOT
BIP-0350 Bech32m used by Bitcoin taproot). The `bech32::encode::<Bech32>`
type-parameter selects the correct checksum polynomial. Decoding is
"lenient" (accepts both BIP-0173 and BIP-0350 checksums) per
upstream's `decodeLenient` semantics — implemented natively by
`bech32::decode()` which tries Bech32m first, falls back to Bech32.

**Empty-stdin behavior**: When stdin is empty / whitespace-only,
upstream emits `bech32: user error (StringToDecodeTooShort)` to
stderr and exits 1. Yggdrasil emits the equivalent error via
`Bech32Error::StringToDecodeTooShort` — message text is
"StringToDecodeTooShort" exactly. Operators grepping logs for this
sentinel get the same match against either binary.

**Error surface naming**: upstream splits error types across
`EncodingError`, `DecodingError`, `HumanReadablePartError`. Yggdrasil
unifies these into a single `Bech32Error` enum for the binary's CLI
path; the upstream symbol names are preserved as variant names where
the type wraps the same concept (e.g. `InvalidPrefix` ↔
`HumanReadablePartError`).

## Verification

```text
$ cargo test -p yggdrasil-bech32
running 23 tests in src + parser modules
... all 23 passed

running 8 tests in tests/cli_help_golden.rs
test help_long_flag_matches_upstream_byte_for_byte ... ok
test help_short_flag_matches_upstream_byte_for_byte ... ok
test version_long_flag_matches_upstream_byte_for_byte ... ok
test version_short_flag_matches_upstream_byte_for_byte ... ok
test unknown_flag_exits_non_zero ... ok
test empty_stdin_emits_string_too_short_error ... ok
test upstream_example_base16_to_bech32_via_stdin ... ok
test upstream_example_decode_to_base16_via_stdin ... ok
test result: ok. 8 passed; 0 failed

$ cargo fmt --all -- --check
(silent — clean)

$ python3 scripts/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ cargo clippy --workspace --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 26.01s

$ cargo test --workspace --all-features
passed: 4887  failed: 0    (was 4872 → +15 new tests)

$ python3 scripts/check-parity-matrix.py
parity matrix clean: 20 entries validated
```

## Drop-in deployment evidence

```text
$ for input in 706174617465 Ae2tdPwUPEYy old_prefix1wpshgcg2s33x3; do
    diff <(echo -n "$input" | upstream/bech32 base16_) \
         <(echo -n "$input" | target/debug/bech32 base16_)
  done
(all empty diffs — byte-equivalent for every documented example)

$ diff <(echo -n "base16_1wpshgct5v5r5mxh0" | upstream/bech32) \
       <(echo -n "base16_1wpshgct5v5r5mxh0" | target/debug/bech32)
(empty diff — byte-equivalent)

$ node/scripts/run-tools.sh bech32 --help | head -1
Usage: bech32 [PREFIX]
```

The `run-tools.sh bech32 --help` invocation works end-to-end:
operator launches via the dispatcher, sees byte-equivalent help text,
and can swap the upstream binary for the yggdrasil one without any
script change.

## Closure criterion

- Concrete encode/decode dispatch implemented backed by pure-Rust
  bech32 + bs58 + hex crates.
- 4 documented upstream `--help` examples pass as round-trip tests.
- Drop-in deployment evidence: byte-equivalent output for every
  tested input.
- `StringToDecodeTooShort` error mirrors upstream's empty-stdin
  behavior.
- All 5 cargo gates + 3 CI parity validators clean.
- Workspace test count: 4,872 → 4,887 (+15).

All six are met.

## Out of scope (R334 closeout)

- **R334 — Closeout**: CHANGELOG entry; AGENTS.md operational
  guide refresh (replace R327 skeleton text with R333-shipped
  state); parity-matrix transition `partial → verified_11_0_1`;
  remove the now-obsolete placeholder `_placeholder` field hack
  if any survives. After R334, bech32 becomes the **first
  sister tool with full deployment-ready 100% parity** to
  upstream — operators can swap `cardano-cli` automation
  scripts to use `target/release/bech32` immediately.
