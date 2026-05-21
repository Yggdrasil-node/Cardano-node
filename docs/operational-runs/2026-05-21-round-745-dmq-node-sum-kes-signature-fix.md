---
title: "Round 745 dmq-node Sum6 KES-signature correction (dmq-node arc, slice 27)"
parent: Reference
---

# Round 745 dmq-node Sum6 KES-signature correction (dmq-node arc, slice 27)

Date: 2026-05-21

## Scope

Slice 27 of the dmq-node arc — a correctness fix discovered while
preparing the cryptographic validator checks.

## The bug

R719 modelled `SigKesSignature` as wrapping `yggdrasil_crypto::KesSignature`
— the 64-byte *base* (single-period) KES signature. But upstream
`SigKESSignature crypto = SigKES (KES crypto)`, and
`Cardano.KESAgent.Protocols.StandardCrypto` fixes
`KES StandardCrypto = Sum6KES Ed25519DSIGN Blake2b_256`. The DMQ KES
signature is therefore a **depth-6 `SumKesSignature`** — `64 + 6 * 64
= 448` bytes — not 64 bytes. The R724 codec compounded this, decoding
the KES-signature field as a fixed 64-byte string.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `DMQ_KES_DEPTH` (`= 6`) — the `Sum6KES` composition depth.
- `SigKesSignature` now wraps `yggdrasil_crypto::SumKesSignature`.
- `encode_sig_raw` encodes the KES signature via
  `SumKesSignature::to_bytes` (the 448-byte raw form);
  `decode_kes_signature` decodes a CBOR byte string into a depth-6
  `SumKesSignature` (`from_bytes(DMQ_KES_DEPTH, …)`), replacing the
  former fixed-64-byte decode.

`crates/tools/dmq-node/src/protocol/local_msg_submission.rs` — the
`dummy_sig` test fixture updated to the Sum6 signature.

1 new test (`sig_kes_signature_is_a_depth_six_sum_kes_signature`)
locks the 448-byte depth-6 shape and its codec round-trip; the R719
/ R721 / R724 tests now exercise the corrected type.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 110 lib (+1 vs R744's 109) +
  2 golden, all green.

## Remaining (dmq-node arc)

- The cryptographic `validate_sig` checks — OCert signature
  verification (`OpCert::verify`) and KES-signature verification of
  the payload (`verify_sum_kes`), now that `SigKesSignature` carries
  the correct `SumKesSignature` type.
- `Configuration/Topology.hs`; the protocol drivers; the
  `Diffusion/*` run-loop wiring; `Tracer.hs`.
