---
title: "Round 633 Typed Conway UTXO DeltaCoin variants (A5 Phase-2.5)"
parent: Reference
---

# Round 633 Typed Conway UTXO DeltaCoin variants (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway UTXO tags 12 (`InsufficientCollateral`) and 20
(`IncorrectTotalCollateralField`) by adding the `DeltaCoinShow`
renderer for the signed `DeltaCoin` payload. After R633, **15
of 23 Conway UTXO variants carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Utxo.hs:110-114,132-137`
  (`InsufficientCollateral DeltaCoin Coin` and
  `IncorrectTotalCollateralField DeltaCoin Coin`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Coin.hs:114-116`
  (`newtype DeltaCoin = DeltaCoin Integer` with `deriving (Show)
  via Quiet DeltaCoin`).

## Changes

- Added `DeltaCoinShow(i64)` renderer matching upstream's Quiet
  Show on `DeltaCoin Integer`: `DeltaCoin <n>` for non-negative,
  `DeltaCoin (-<n>)` for negative (the inner Integer is shown at
  precedence 11, so negatives are parenthesised).
- Refactored `ConwayUtxoPredFailure` variants 12/20 from
  `Vec<u8>` to struct shapes:
  - Tag 12 `InsufficientCollateral { balance: i64, required:
    u64 }`.
  - Tag 20 `IncorrectTotalCollateralField { provided: i64,
    declared: u64 }`.
- `from_cbor` decodes both as 3-element envelopes `[tag,
  DeltaCoin (signed), Coin (unsigned)]`. The DeltaCoin field
  uses the decoder's `signed()` reader (CBOR major-type 0/1).
- Display routes both through `DeltaCoinShow` + `CoinShow`.

2 new focused unit tests:
- `_insufficient_collateral_tag12` — negative DeltaCoin
  (balance -500) exercising the parenthesised render.
- `_incorrect_total_collateral_tag20` — positive DeltaCoin.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (283 lib + 4
  doctests + 1 main, +2 new tests vs R632 baseline of 281)

## Remaining (A5 Phase-2.5+)

- Conway UTXO raw variants: tag 2 (ValidityInterval+SlotNo), 6
  (Value Mismatch), 11 (Int/Int/TxOut triple), 13 (NonEmptyMap
  TxIn TxOut), 14 (ExUnits Mismatch), 15 (Value), 21
  (TxOut/Coin pair), 22 (NonEmpty TxIn).
- Conway UTXOW raw variants (tags 10/13/15/18).
- Conway UTXOS `CollectErrors`.
- Conway GOV raw variants (18 governance-specific decoders).
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
