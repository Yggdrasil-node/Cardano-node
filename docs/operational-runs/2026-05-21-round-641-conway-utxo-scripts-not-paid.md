---
title: "Round 641 Typed Conway UTXO ScriptsNotPaidUTxO variant (A5 Phase-2.5)"
parent: Reference
---

# Round 641 Typed Conway UTXO ScriptsNotPaidUTxO variant (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway UTXO tag 13 (`ScriptsNotPaidUTxO`) by adding the
`NonEmptyMapTxInTxOut` carrier. After R641, **21 of 23 Conway
UTXO variants carry typed payloads** — only tags 6/15 (multi-
asset Value) remain raw.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Utxo.hs:115-117,326`
  (`ScriptsNotPaidUTxO (NonEmptyMap TxIn (TxOut era))`, CBOR
  `Sum` tag 13).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-data/src/Data/Map/NonEmpty.hs:23-25`
  (`newtype NonEmptyMap k v = NonEmptyMap (Map k v)` with
  `deriving stock (Show, Eq)`, `deriving newtype (EncCBOR, ...)`
  → CBOR map encoding).

## Changes

- Added `NonEmptyMapTxInTxOut` carrier — a `Vec<(TxIn,
  ShelleyTxOut)>` preserving wire order. CBOR wire format is a
  CBOR map (TxIn key → Shelley TxOut value). Empty maps reject
  at decode time. Display matches upstream stock-derived `Show
  (NonEmptyMap k v)`: `NonEmptyMap (fromList [(<k>, <v>),
  ...])`.
- Refactored `ConwayUtxoPredFailure::ScriptsNotPaidUTxO(Vec<u8>)`
  → `ScriptsNotPaidUTxO(NonEmptyMapTxInTxOut)`. `from_cbor`
  decodes the 2-element envelope `[13, NonEmptyMap TxIn TxOut]`.
  Display routes the typed payload.

2 new focused unit tests:
- `_scripts_not_paid_tag13` — single-entry map round-trip
  asserting the typed Display chain.
- `_scripts_not_paid_rejects_empty` — empty-map rejection.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (297 lib + 4
  doctests + 1 main, +2 new tests vs R640 baseline of 295)

## Remaining (A5 Phase-2.5+)

- Conway UTXO raw variants: tag 6 (ValueNotConservedUTxO —
  Mismatch over the era-specific multi-asset `Value`), tag 15
  (CollateralContainsNonADA — `Value`). Both require the
  Mary-era MultiAsset `Value` decoder.
- Conway UTXOW tag 10 (MissingRedeemers — PlutusPurpose AsItem).
- Conway UTXOS `CollectErrors`.
- Conway GOV raw variants (18 governance-specific decoders).
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
