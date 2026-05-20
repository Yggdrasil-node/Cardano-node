---
title: "Round 603 NonEmptySet TxIn decoder for ShelleyUtxoPredFailure tag 0 (A5 Phase-2.5)"
parent: Reference
---

# Round 603 NonEmptySet TxIn decoder for ShelleyUtxoPredFailure tag 0 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `TxIn` (TxId + TxIx) record + `NonEmptySetTxIn` carrier
+ decoder, wires `ShelleyUtxoPredFailure::BadInputsUTxO` (tag 0)
to typed payload. After R603, 5 of 11 `ShelleyUtxoPredFailure`
variants carry typed payloads.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/TxIn.hs:59-108`
  (`newtype TxId`, `data TxIn = TxIn !TxId !TxIx`, CBOR encode as
  2-element list).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/BaseTypes.hs:911-914`
  (`newtype TxIx = TxIx {unTxIx :: Word16}` stock-derived Show).

## Changes

- Added `TxId([u8; 32])` newtype with Display matching upstream
  stock-derived record Show: `TxId {unTxId = SafeHash "<hex>"}`.
- Added `TxIx(u16)` newtype with Display matching upstream
  stock-derived record Show: `TxIx {unTxIx = <n>}`.
- Added `TxIn { tx_id: TxId, tx_ix: TxIx }` record with
  `from_decoder` helper that reads a 2-element CBOR array
  (id bytes(32), ix Word16 with decode-time narrowing). Display
  matches upstream stock-derived constructor Show:
  `TxIn (TxId {...}) (TxIx {...})` — each single-arg constructor
  wrapped in parens at showsPrec 11.
- Added `NonEmptySetTxIn` struct (`BTreeSet<TxIn>` so ordering
  follows upstream Ord TxIn comparing by TxId then TxIx) with
  `from_cbor` decoder. Tag-258 tolerant, rejects empty sets.
- Display: `NonEmptySet (fromList [<TxIn>, ...])`.
- Refactored `ShelleyUtxoPredFailure::BadInputsUTxO(Vec<u8>)` →
  `BadInputsUTxO(NonEmptySetTxIn)`. Updated `from_cbor` dispatcher
  and Display routing.
- Replaced R602's tag-0-raw-routing test with a tag-5 equivalent
  (ValueNotConservedUTxO — still raw pending Value decoder).

3 new focused unit tests:
- `_bad_inputs_decodes_tag0` end-to-end typed decode with 1
  TxIn entry; verifies TxId bytes, TxIx value, and full Display
  shape.
- `non_empty_set_tx_in_rejects_empty_set` (NonEmpty invariant).
- `_routes_tag5_to_raw_variant` (replaces the prior tag-0 raw
  test).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint` (fixed a `clippy::manual_range_patterns` warning
  by collapsing `5 | 6 | 7 | 8 | 9 | 10` to `5..=10`)
- `cargo test -p yggdrasil-cardano-submit-api` (185 lib + 4
  doctests + 1 main, +2 net new tests vs R602 baseline of 183 —
  added 3, replaced 1)

## Remaining (A5 Phase-2.5+)

- Era-specific `Mismatch<Value era>` decoder for tag 5
  (Shelley=Coin, Mary+=MultiAsset).
- `NonEmpty (TxOut era)` decoder for tags 6 and 10.
- `ShelleyPpupPredFailure` decoder for tag 7.
- Network + `NonEmptySet Addr` decoders for tag 8.
- Network + `NonEmptySet AccountAddress` decoders for tag 9.
- Wire typed `ShelleyUtxoPredFailure` into
  `ShelleyUtxowPredFailure::UtxoFailure(Vec<u8>)` (tag 4).
- Wire typed `ShelleyUtxowPredFailure` into
  `ShelleyLedgerPredFailure::UtxowFailure(Vec<u8>)` (LEDGER tag 0).
- `ShelleyDelegsPredFailure` decoder for LEDGER tag 1.
- Mirror predicate-failure tree for Allegra/Mary/Alonzo/Babbage/
  Conway eras.
