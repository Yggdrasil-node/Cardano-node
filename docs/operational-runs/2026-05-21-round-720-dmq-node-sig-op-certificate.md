---
title: "Round 720 dmq-node SigOpCertificate (dmq-node arc, slice 4)"
parent: Reference
---

# Round 720 dmq-node SigOpCertificate (dmq-node arc, slice 4)

Date: 2026-05-21

## Scope

Slice 4 of the dmq-node arc. Adds the `SigOpCertificate` newtype to
`protocol/sig_submission.rs`.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `SigOpCertificate` — wraps `yggdrasil_consensus::OpCert`. Mirror of
  upstream `newtype SigOpCertificate crypto = SigOpCertificate
  (OCert crypto)`. The `crypto` parameter collapses to the concrete
  consensus operational certificate (hot KES verkey / counter /
  KES-period / cold signature) shared with block-header validation.

`crates/tools/dmq-node/Cargo.toml` — new workspace-internal
dependency `yggdrasil-consensus`.

1 unit test covers wrapping, equality, and the `get` accessor.

This completes the three crypto-parameterized `SigSubmission`
newtypes (`SigKesSignature` R719, `SigColdKey` R719,
`SigOpCertificate` R720).

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 58 lib (+1 vs R719's 57) +
  2 golden, all green.

## Remaining (dmq-node arc)

- `SigRaw` / `SigRawWithSignedBytes` / `Sig` — the composed payload
  types, plus a `PosixTime` model for `sigRawExpiresAt` (upstream
  `POSIXTime`).
- `SigValidationError` `ToJSON` rendering.
- `SigSubmission` CBOR codec (warrants a `parity-plan`) + validator.
- NodeToClient / NodeToNode protocols, Diffusion wiring.
