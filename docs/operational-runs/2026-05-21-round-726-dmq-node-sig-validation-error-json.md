---
title: "Round 726 dmq-node SigValidationError JSON (dmq-node arc, slice 10)"
parent: Reference
---

# Round 726 dmq-node SigValidationError JSON (dmq-node arc, slice 10)

Date: 2026-05-21

## Scope

Slice 10 of the dmq-node arc. Adds `SigValidationError::to_json` —
the `ToJSON` rendering deferred at R718.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `SigValidationError::to_json` — mirror of upstream
  `instance ToJSON SigValidationError`: `SigDuplicate` / `SigExpired`
  render as the bare JSON strings `"duplicate"` / `"expired"`;
  `SigResultOther` as `{"type":"other","reason":<text>}`; every other
  variant as `{"type":"invalid","reason":<rendered error>}`.

The JSON *structure* is byte-exact with upstream. Upstream's
`"invalid"` `reason` uses Haskell `show`; the Rust port uses the
variant's `Debug` rendering — for the field-less variants
(`ClockSkew`, `PoolNotEligible`, …) that is identical, and for the
field-bearing variants the human-readable `reason` text is the Rust
formatting of the same fields (documented divergence on a
logging-only string).

1 unit test covers all four JSON shapes.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 68 lib (+1 vs R725's 67) +
  2 golden, all green.

## Remaining (dmq-node arc)

- The `codecSigSubmission` TxSubmission2 wrapper +
  `timeLimitsSigSubmission` / `byteLimitsSigSubmission` tables — a
  `yggdrasil-network` integration sub-arc (`SigSubmission =
  TxSubmission2 SigId Sig`).
- The signature validator (`Validate.hs`).
- NodeToClient / NodeToNode protocols; Diffusion wiring.
