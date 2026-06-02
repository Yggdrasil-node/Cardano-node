---
title: "Round 719 dmq-node SigSubmission crypto newtypes (dmq-node arc, slice 3)"
parent: Reference
---

# Round 719 dmq-node SigSubmission crypto newtypes (dmq-node arc, slice 3)

Date: 2026-05-21

## Scope

Slice 3 of the dmq-node arc. Adds the two `yggdrasil-crypto`-backed
`SigSubmission` newtypes to `protocol/sig_submission.rs`.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `SigKesSignature` — wraps `yggdrasil_crypto::KesSignature`. Mirror
  of upstream `newtype SigKESSignature crypto = SigKESSignature
  (SigKES (KES crypto))`.
- `SigColdKey` — wraps `yggdrasil_crypto::VerificationKey` (Ed25519).
  Mirror of upstream `newtype SigColdKey crypto = SigColdKey
  (VerKeyDSIGN (KES.DSIGN crypto))`.

Upstream parameterizes these over a `crypto` type variable; yggdrasil
is not generic over the crypto suite, so the parameter collapses to
the concrete yggdrasil crypto types.

`crates/tools/dmq-node/Cargo.toml` — new workspace-internal
dependency `yggdrasil-crypto`.

2 unit tests cover wrapping, equality, and the `get` accessors.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 57 lib (+2 vs R718's 55) +
  2 golden, all green.

## Remaining (dmq-node arc)

- `SigOpCertificate` — wraps the consensus operational certificate
  (`crates/consensus/src/ocert`); adds a `yggdrasil-consensus` dep.
- `SigRaw` / `SigRawWithSignedBytes` / `Sig` — the composed payload
  types, plus a `POSIXTime` model for `sigRawExpiresAt`.
- `SigValidationError` `ToJSON` rendering.
- `SigSubmission` CBOR codec (warrants a `parity-plan`) + validator.
- NodeToClient / NodeToNode protocols, Diffusion wiring.
