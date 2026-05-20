---
title: "Round 601 NonEmpty VKey Witness decoder for UTXOW tag 0 (A5 Phase-2.5)"
parent: Reference
---

# Round 601 NonEmpty VKey Witness decoder for UTXOW tag 0 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `NonEmpty (VKey Witness)` typed decoder and wires it into
`ShelleyUtxowPredFailure::InvalidWitnessesUTXOW` (tag 0). After
this round, **10 of the 11 `ShelleyUtxowPredFailure` variants
carry fully-typed payloads** — only tag 4 (`UtxoFailure`, nested
`ShelleyUtxoPredFailure` sub-rule) remains raw.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Keys/Internal.hs:115-118`
  (`newtype VKey kd` + `deriving via Quiet (VKey kd) instance
  Show (VKey kd)`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs:113,178-179`
  (tag 0 `InvalidWitnessesUTXOW (NonEmpty (VKey Witness))`).

## Changes

- Added `VKey([u8; 32])` newtype with Display matching upstream
  Quiet-derived shape: `VKey (VerKeyEd25519DSIGN "<hex>")`. The
  phantom `kd :: KeyRole` is collapsed since it doesn't affect
  wire format or Show.
- Added `NonEmptyVKey` struct (`Vec<VKey>` preserving insertion
  order — `NonEmpty` is sequential, not a set) with `from_cbor`
  decoder that reads a regular CBOR array and rejects empty
  arrays at decode time per the NonEmpty invariant.
- Display impl: `<head> :| [<tail-comma-separated>]` matching
  upstream `Show (NonEmpty a)`. Single-entry case renders as
  `<head> :| []`.
- Refactored `ShelleyUtxowPredFailure::InvalidWitnessesUTXOW(Vec<u8>)`
  → `InvalidWitnessesUTXOW(NonEmptyVKey)`.
- Updated `from_cbor` dispatcher: tag 0 routes through
  `NonEmptyVKey::from_cbor`; tag 4 remains the only raw-payload
  variant.
- Updated Display: typed routing for tag 0; raw-cbor marker only
  for tag 4.
- Replaced the R600 tag-0 raw-routing test with a tag-4
  equivalent (only remaining raw variant).

3 new focused unit tests:
- `_invalid_witnesses_decodes_tag0` end-to-end typed decode.
- `non_empty_vkey_rejects_empty_list` (NonEmpty invariant).
- `non_empty_vkey_multi_entry_renders_with_cons_separator`
  (`:|` separator in Display).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (177 lib + 4
  doctests + 1 main, +3 new tests vs R600 baseline of 174)

## Remaining (A5 Phase-2.5+)

- `ShelleyUtxoPredFailure` decoder for tag 4 (the biggest
  sub-rule — ~13 variants including BadInputsUTxO,
  ValueNotConservedUTxO, OutputBootAddrAttrsTooBig,
  FeeTooSmallUTxO, MaxTxSizeUTxO, ...).
- Wire the typed `ShelleyUtxowPredFailure` into
  `ShelleyLedgerPredFailure::UtxowFailure(Vec<u8>)`.
- `ShelleyDelegsPredFailure` decoder for tag-1 of the LEDGER tree.
- Mirror the predicate-failure tree for Allegra/Mary/Alonzo/
  Babbage/Conway eras.
