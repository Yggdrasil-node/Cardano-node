---
title: "Round 636 Typed Conway UTXOW ExtraRedeemers variant (A5 Phase-2.5)"
parent: Reference
---

# Round 636 Typed Conway UTXOW ExtraRedeemers variant (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway UTXOW tag 15 (`ExtraRedeemers`) by adding the
`ConwayPlutusPurposeIx` enum (the index-only `AsIx` form of the
Conway Plutus script purpose) and the `NonEmptyPlutusPurposeIx`
carrier. After R636, **15 of 19 Conway UTXOW variants carry
typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Scripts.hs:202-276`
  (`data ConwayPlutusPurpose f era` 6-variant ADT; `EncCBORGroup`
  with `listLen = 2`, `encCBORGroup` emits `encodeWord8 <tag> <>
  encCBOR p`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Scripts.hs:257-259`
  (`newtype AsIx ix it = AsIx {unAsIx :: ix}` — record `Show`,
  `EncCBOR` derived newtype so it encodes as just the index).

## Changes

- Added `ConwayPlutusPurposeIx` enum — the `ConwayPlutusPurpose
  AsIx` form: 6 variants (ConwaySpending / ConwayMinting /
  ConwayCertifying / ConwayRewarding / ConwayVoting /
  ConwayProposing), each carrying a `u32` redeemer pointer.
  `from_decoder` reads the 2-element CBORGroup `[word8-tag,
  word32-index]`. Display: `<Constructor> (AsIx {unAsIx =
  <n>})`.
- Added `NonEmptyPlutusPurposeIx` carrier — a `Vec`-based
  NonEmpty list of `ConwayPlutusPurposeIx`. CBOR wire format is
  a plain array of 2-element group envelopes. Empty arrays
  reject at decode time. Display: `<head> :| [<tail>...]`.
- Refactored `ConwayUtxowPredFailure::ExtraRedeemers(Vec<u8>)` →
  `ExtraRedeemers(NonEmptyPlutusPurposeIx)`. `from_cbor` decodes
  the 2-element envelope `[15, NonEmpty (PlutusPurpose AsIx)]`.
  Display routes the typed payload.

3 new focused unit tests:
- `_extra_redeemers_tag15` — 2-purpose round-trip (Spending +
  Minting) with the full Display chain.
- `_extra_redeemers_rejects_empty` — NonEmpty empty-array
  rejection.
- `conway_plutus_purpose_ix_covers_all_six_purposes` —
  parameterized test over all 6 purpose tags.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (290 lib + 4
  doctests + 1 main, +3 new tests vs R635 baseline of 287)

## Remaining (A5 Phase-2.5+)

- Conway UTXOW raw variants: tag 10 (MissingRedeemers —
  `NonEmpty (PlutusPurpose AsItem, ScriptHash)`, the AsItem
  form carries the actual TxIn/PolicyID/TxCert/etc. item), tag
  13 (PPViewHashesDontMatch), tag 18
  (ScriptIntegrityHashMismatch).
- Conway UTXO raw variants (tags 6/13/14/15/21).
- Conway UTXOS `CollectErrors`.
- Conway GOV raw variants (18 governance-specific decoders).
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
