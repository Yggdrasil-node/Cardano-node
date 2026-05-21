---
title: "Round 634 Typed Conway UTXO ValidityInterval variant (A5 Phase-2.5)"
parent: Reference
---

# Round 634 Typed Conway UTXO ValidityInterval variant (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway UTXO tag 2 (`OutsideValidityIntervalUTxO`) by
adding the `StrictMaybeSlot` and `ValidityInterval` types. After
R634, **16 of 23 Conway UTXO variants carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/allegra/impl/src/Cardano/Ledger/Allegra/Scripts.hs:119-150`
  (`data ValidityInterval = ValidityInterval { invalidBefore ::
  StrictMaybe SlotNo, invalidHereafter :: StrictMaybe SlotNo }`;
  CBOR `Rec ValidityInterval !> To f !> To t` = 2-element record
  array).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-binary/src/Cardano/Ledger/Binary/Encoding/Encoder.hs:324-327`
  (`encodeStrictMaybe`: `SNothing -> encodeListLen 0`,
  `SJust x -> encodeListLen 1 <> encodeValue x`).

## Changes

- Added `StrictMaybeSlot(Option<u64>)` — decodes a `StrictMaybe
  SlotNo` from a CBOR list (0-element = SNothing, 1-element =
  SJust). Display: `SNothing` / `SJust (SlotNo {unSlotNo =
  <n>})`.
- Added `ValidityInterval { invalid_before: StrictMaybeSlot,
  invalid_hereafter: StrictMaybeSlot }` — decodes the 2-element
  record array. Display matches upstream stock-derived record
  Show: `ValidityInterval {invalidBefore = <SMaybe>,
  invalidHereafter = <SMaybe>}`.
- Refactored `ConwayUtxoPredFailure::OutsideValidityIntervalUTxO(Vec<u8>)`
  → struct variant `{ interval: ValidityInterval, current_slot:
  u64 }`. `from_cbor` decodes the 3-element envelope `[2,
  ValidityInterval, SlotNo]`. Display:
  `OutsideValidityIntervalUTxO (<interval>) (SlotNo {unSlotNo =
  <n>})`.

2 new focused unit tests:
- `_outside_validity_interval_tag2` — both bounds SJust.
- `_outside_validity_interval_open_bounds_tag2` — both bounds
  SNothing (open interval).

Lint cleanup: removed a duplicated doc comment left at the
insertion anchor.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (285 lib + 4
  doctests + 1 main, +2 new tests vs R633 baseline of 283)

## Remaining (A5 Phase-2.5+)

- Conway UTXO raw variants: tag 6 (Value Mismatch), 13
  (NonEmptyMap TxIn TxOut), 14 (ExUnits Mismatch), 15 (Value),
  21 (TxOut/Coin pair), 22 (NonEmpty TxIn).
- Conway UTXOW raw variants (tags 10/13/15/18).
- Conway UTXOS `CollectErrors`.
- Conway GOV raw variants (18 governance-specific decoders).
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
