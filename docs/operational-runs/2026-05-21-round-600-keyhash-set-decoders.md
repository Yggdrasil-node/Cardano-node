---
title: "Round 600 KeyHash + NonEmptySet/Set decoders for UTXOW tags 1/5 (A5 Phase-2.5)"
parent: Reference
---

# Round 600 KeyHash + NonEmptySet/Set decoders for UTXOW tags 1/5 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Continues A5 Phase-2.5 by adding the `KeyHash` newtype and
`NonEmptySetKeyHash` / `SetKeyHash` carriers, wiring them into
`ShelleyUtxowPredFailure` tags 1 (`MissingVKeyWitnessesUTXOW`) and
5 (`MIRInsufficientGenesisSigsUTXOW`). After R600 only tags 0 and
4 remain raw within the `ShelleyUtxowPredFailure` enum (2 of the
original 11 variants).

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Hashes.hs:162-180`
  (`newtype KeyHash (r :: KeyRole) = KeyHash {unKeyHash :: Hash
  ADDRHASH (VerKeyDSIGN DSIGN)}` — 28-byte hash with record-syntax
  Show).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs:115-122,180-198`
  (tag 1 `MissingVKeyWitnessesUTXOW (NonEmptySet (KeyHash
  Witness))`, tag 5 `MIRInsufficientGenesisSigsUTXOW (Set (KeyHash
  Witness))`).

## Changes

- Added `KeyHash([u8; 28])` newtype with Display matching upstream
  record Show: `KeyHash {unKeyHash = "<hex>"}`.
- Added `NonEmptySetKeyHash` struct (`BTreeSet<KeyHash>`) with
  tag-258 tolerant `from_cbor` decoder. Rejects empty sets to
  honour the NonEmpty invariant.
- Added `SetKeyHash` struct (`BTreeSet<KeyHash>`) with tag-258
  tolerant `from_cbor` decoder that permits empty sets (matches
  upstream's raw `Set` type used by `MIRInsufficientGenesisSigsUTXOW`).
- Display:
  - `NonEmptySetKeyHash`: `NonEmptySet (fromList [KeyHash
    {unKeyHash = "..."}, ...])` matching upstream
    `deriving stock Show` on the `NonEmptySet` newtype.
  - `SetKeyHash`: bare `fromList [KeyHash {unKeyHash = "..."},
    ...]` matching upstream `Show (Set a)` without a NonEmptySet
    wrapper.
- Refactored variant payloads:
  - `MissingVKeyWitnessesUTXOW(Vec<u8>)` →
    `MissingVKeyWitnessesUTXOW(NonEmptySetKeyHash)`.
  - `MIRInsufficientGenesisSigsUTXOW(Vec<u8>)` →
    `MIRInsufficientGenesisSigsUTXOW(SetKeyHash)`.
- Updated `from_cbor` dispatcher: tag 1 routes through
  `NonEmptySetKeyHash`, tag 5 routes through `SetKeyHash`. Tags 0
  and 4 remain on the raw-bytes carrier pending NonEmpty (VKey
  Witness) and ShelleyUtxoPredFailure decoders.
- Updated Display: typed routing for tags 1 and 5; raw-cbor
  marker for tags 0 and 4 only.
- Replaced the R598 `_routes_unported_tag_to_raw_variant` test
  (which used tag 1) with a tag-0 equivalent.

3 new focused unit tests:
- `_missing_vkey_witnesses_decodes_tag1` end-to-end.
- `_mir_insufficient_genesis_sigs_decodes_tag5_empty_set`
  (empty-set acceptance — distinguishes Set from NonEmptySet).
- `_mir_insufficient_genesis_sigs_decodes_tag5_with_keys`
  (multi-key Display ordering).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (174 lib + 4
  doctests + 1 main, +3 new tests vs R599 baseline of 171)

## Remaining (A5 Phase-2.5+)

- `NonEmpty (VKey Witness)` decoder for tag 0
  (`InvalidWitnessesUTXOW`). VKey wraps a 32-byte ed25519 verkey.
- `ShelleyUtxoPredFailure` decoder for tag 4 (nested sub-rule —
  its own multi-variant enum with witness/UTxO-balance-related
  failures).
- Wire the typed `ShelleyUtxowPredFailure` into
  `ShelleyLedgerPredFailure::UtxowFailure(Vec<u8>)`.
- Mirror the predicate-failure tree for Allegra/Mary/Alonzo/Babbage/
  Conway eras (Conway adds 4+ governance-specific variants).
