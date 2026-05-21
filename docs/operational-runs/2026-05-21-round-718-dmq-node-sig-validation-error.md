---
title: "Round 718 dmq-node SigValidationError tree (dmq-node arc, slice 2)"
parent: Reference
---

# Round 718 dmq-node SigValidationError tree (dmq-node arc, slice 2)

Date: 2026-05-21

## Scope

Slice 2 of the dmq-node arc. Appends the `SigSubmission` validation
error/trace/exception types to `protocol/sig_submission.rs`.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `SigValidationError` — 12-variant enum mirroring upstream
  `data SigValidationError` (`SigSubmission/Type.hs`): the four
  KES/OpCert-verification failures (`InvalidKESSignature`,
  `InvalidSignatureOCERT`, `InvalidOCertCounter`, plus the
  `KESBeforeStartOCERT` / `KESAfterEndOCERT` window checks), the
  pool-eligibility failures, and the duplicate / expired / clock-skew
  / other terminal reasons.
- `SigValidationTrace` — `InvalidSignature SigId SigValidationError`.
- `SigValidationException` — the `thiserror::Error` analog of
  upstream's `instance Exception SigValidationException`.

KES periods are `u64`: upstream's `KESPeriod` is a `Word` newtype and
CIP-137 mandates `Word64` for DMQ KES periods (the `Type.hs`
`sigRawKESPeriod` note). Multi-field variants use named fields for
caller safety; the variant names mirror upstream constructors. The
dependency-free `u64`/`String` modelling keeps this slice self-contained
— the typed crypto surface (KES signatures, OpCert, cold keys) arrives
with the `SigRaw` / `Sig` payload slice, which adds the
`yggdrasil-crypto` dependency.

3 unit tests cover variant construction/equality, the trace's
id+error payload, and the exception's `Display`.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 55 lib (+3 vs R717's 52) +
  2 golden, all green.

## Remaining (dmq-node arc)

- `SigRaw` / `Sig` crypto-parameterized payload types (KES signature,
  OpCert, cold key) — adds the `yggdrasil-crypto` dependency.
- `SigValidationError` `ToJSON` rendering.
- `SigSubmission` CBOR codec (warrants a `parity-plan`).
- `SigSubmission` validator (`Validate.hs`).
- NodeToClient / NodeToNode protocols, Diffusion wiring.
