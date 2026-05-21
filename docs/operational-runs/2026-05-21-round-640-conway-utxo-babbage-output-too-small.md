---
title: "Round 640 Typed Conway UTXO BabbageOutputTooSmallUTxO variant (A5 Phase-2.5)"
parent: Reference
---

# Round 640 Typed Conway UTXO BabbageOutputTooSmallUTxO variant (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway UTXO tag 21 (`BabbageOutputTooSmallUTxO`) by adding
the `NonEmptyTxOutCoinPair` carrier. After R640, **20 of 23
Conway UTXO variants carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Utxo.hs:138-141,334,368`
  (`BabbageOutputTooSmallUTxO (NonEmpty (TxOut era, Coin))` —
  CBOR `Sum` tag 21, payload is a `NonEmpty` of `(TxOut, Coin)`
  pairs).

## Changes

- Added `NonEmptyTxOutCoinPair` carrier — a `Vec<(ShelleyTxOut,
  u64)>` preserving wire order. Each pair is a 2-element CBOR
  array `[TxOut, Coin]`; the outer payload is a plain CBOR array
  of such pairs. Empty arrays reject at decode time. Display
  matches upstream `Show (NonEmpty (TxOut, Coin))`: `<head> :|
  [<tail>...]` where each pair is the Haskell tuple `(<TxOut>,
  Coin <n>)` (and `<TxOut>` itself renders as the
  `(<Addr>, Coin <m>)` tuple — yielding the nested-paren shape).
- Refactored `ConwayUtxoPredFailure::BabbageOutputTooSmallUTxO(Vec<u8>)`
  → `BabbageOutputTooSmallUTxO(NonEmptyTxOutCoinPair)`.
  `from_cbor` decodes the 2-element envelope `[21, NonEmpty
  (TxOut, Coin)]`. Display routes the typed payload.

2 new focused unit tests:
- `_babbage_output_too_small_tag21` — single-pair round-trip
  asserting the nested-tuple Display shape.
- `_babbage_output_too_small_rejects_empty` — NonEmpty
  empty-array rejection.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (295 lib + 4
  doctests + 1 main, +2 new tests vs R639 baseline of 293)

## Remaining (A5 Phase-2.5+)

- Conway UTXO raw variants: tag 6 (ValueNotConservedUTxO —
  Mismatch Value), 13 (ScriptsNotPaidUTxO — NonEmptyMap TxIn
  TxOut), 15 (CollateralContainsNonADA — Value).
- Conway UTXOW tag 10 (MissingRedeemers — PlutusPurpose AsItem).
- Conway UTXOS `CollectErrors`.
- Conway GOV raw variants (18 governance-specific decoders).
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
