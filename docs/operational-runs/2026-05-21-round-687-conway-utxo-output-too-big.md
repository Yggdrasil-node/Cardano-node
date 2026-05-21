---
title: "Round 687 Typed Conway UTXO OutputTooBigUTxO (A5 Phase-2.5)"
parent: Reference
---

# Round 687 Typed Conway UTXO OutputTooBigUTxO (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway UTXO tag 11 (`OutputTooBigUTxO`) — the last raw
`Vec<u8>` payload in the `ConwayUtxoPredFailure` enum. Every
variant of all 23 Conway UTXO predicate failures now carries a
fully typed payload.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Utxo.hs:107-109,324,358`
  (`OutputTooBigUTxO (NonEmpty (Int, Int, TxOut era))` — a
  non-empty list of `(actualSize, maxValue, TxOut)` triples;
  CBOR `Sum` tag 11).

## Changes

- Added `NonEmptyOutputTooBig` carrier — a `Vec<(i64, i64,
  ShelleyTxOut)>` decoding a non-empty CBOR array of 3-element
  `[actualSize, maxValue, TxOut]` arrays; empty arrays reject
  at decode time. Display: `<head> :| [<tail>...]` with each
  entry a Haskell 3-tuple.
- Refactored `ConwayUtxoPredFailure::OutputTooBigUTxO(Vec<u8>)`
  → `OutputTooBigUTxO(NonEmptyOutputTooBig)`. `from_cbor`
  decodes the 2-element envelope `[11, NonEmpty (Int, Int,
  TxOut)]`.
- Removed the now-unused `capture_raw` closure from
  `ConwayUtxoPredFailure::from_cbor` — every UTXO variant is
  typed.

2 new focused unit tests:
- `_output_too_big_tag11` — single-triple round-trip asserting
  the typed sizes and TxOut.
- `_output_too_big_rejects_empty` — NonEmpty empty-array
  rejection.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (355 lib + 4
  doctests + 1 main, +2 new tests vs R686 baseline of 353)

## Remaining (A5 Phase-2.5+)

- `PParamValue::Raw` remains as a tolerant fallback for
  unexpected protocol-parameter value shapes (by design).
- Deeper pre-Conway DELEGS payload coverage (the typed Shelley
  DELEGS → DELPL → POOL/DELEG chain is already complete).
