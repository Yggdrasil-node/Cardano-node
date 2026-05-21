---
title: "Round 635 Typed Conway UTXO NonEmpty-TxIn variant (A5 Phase-2.5)"
parent: Reference
---

# Round 635 Typed Conway UTXO NonEmpty-TxIn variant (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway UTXO tag 22 (`BabbageNonDisjointRefInputs`) by
adding the `NonEmptyTxIn` carrier. After R635, **18 of 23 Conway
UTXO variants carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Utxo.hs:142-144,335,369`
  (`BabbageNonDisjointRefInputs (NonEmpty TxIn)` — CBOR `Sum`
  tag 22, payload is a plain `NonEmpty TxIn`).

## Changes

- Added `NonEmptyTxIn` carrier — a `Vec<TxIn>` preserving wire
  order (unlike `NonEmptySetTxIn`'s BTreeSet, which dedups and
  sorts). The CBOR wire format is a plain CBOR array with no
  tag-258 prefix (a `NonEmpty` list, not a `NonEmptySet`). Empty
  arrays are rejected at decode time. Display matches upstream
  `Show (NonEmpty TxIn)`: `<head> :| [<tail>...]`.
- Refactored `ConwayUtxoPredFailure::BabbageNonDisjointRefInputs(Vec<u8>)`
  → `BabbageNonDisjointRefInputs(NonEmptyTxIn)`. `from_cbor`
  decodes the 2-element envelope `[22, NonEmpty TxIn]`. Display
  routes the typed payload.

2 new focused unit tests:
- `_babbage_non_disjoint_ref_inputs_tag22` — single-TxIn
  round-trip + Display.
- `_babbage_non_disjoint_ref_inputs_rejects_empty` — NonEmpty
  empty-array rejection.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (287 lib + 4
  doctests + 1 main, +2 new tests vs R634 baseline of 285)

## Remaining (A5 Phase-2.5+)

- Conway UTXO raw variants: tag 6 (Value Mismatch), 13
  (NonEmptyMap TxIn TxOut), 14 (ExUnits Mismatch), 15 (Value),
  21 (NonEmpty (TxOut, Coin) pair).
- Conway UTXOW raw variants (tags 10/13/15/18).
- Conway UTXOS `CollectErrors`.
- Conway GOV raw variants (18 governance-specific decoders).
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
