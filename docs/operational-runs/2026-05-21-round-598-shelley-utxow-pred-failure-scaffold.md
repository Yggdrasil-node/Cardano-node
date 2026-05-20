---
title: "Round 598 ShelleyUtxowPredFailure scaffold (A5 Phase-2.5)"
parent: Reference
---

# Round 598 ShelleyUtxowPredFailure scaffold (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `ShelleyUtxowPredFailure` 11-variant enum + CBOR decoder
for the simple-payload tags (6/7/8/9). The remaining variants
(0/1/2/3/4/5/10) carry raw inner CBOR pending per-variant typed
decoders that need NonEmptySet / NonEmpty / sub-rule support.

This is the first sub-rule under `ShelleyLedgerPredFailure::UtxowFailure`
to receive a typed decoder; the parent variant still carries raw
bytes pending the R598 follow-on round that wires the typed
sub-rule.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs:112-133`
  (`data ShelleyUtxowPredFailure era` 11-variant ADT).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs:171-209`
  (`encCBOR` — outer 2-element array for tags 0..8,10; 1-element
  array for tag 9 `InvalidMetadata`).

## Changes

- Added `TxAuxDataHash` 32-byte newtype with Display matching
  upstream `Show TxAuxDataHash`: `TxAuxDataHash {unTxAuxDataHash =
  SafeHash "<hex>"}`.
- Added `ShelleyUtxowPredFailure` 11-variant enum:
  - Tags 0/1/2/3 (witness-set NonEmpty(Set) variants): raw payload
    pending NonEmptySet decoder.
  - Tag 4 `UtxoFailure`: raw payload pending nested
    `ShelleyUtxoPredFailure` decoder.
  - Tag 5 `MIRInsufficientGenesisSigsUTXOW`: raw payload pending
    `Set (KeyHash Witness)` decoder.
  - Tag 6 `MissingTxBodyMetadataHash(TxAuxDataHash)`: typed.
  - Tag 7 `MissingTxMetadata(TxAuxDataHash)`: typed.
  - Tag 8 `ConflictingMetadataHash(Mismatch<TxAuxDataHash>)`: typed
    (reuses R597's generic `Mismatch<T>`).
  - Tag 9 `InvalidMetadata`: payload-free; uses upstream's
    1-element-array CBOR envelope.
  - Tag 10 `ExtraneousScriptWitnessesUTXOW`: raw pending decoder.
- Added `tag()`, `constructor()`, Display impl matching upstream
  stock-derived Show.
- Added `from_cbor` decoder that walks the outer CBOR array,
  reads the Word8 tag, dispatches per-variant. Length-1 vs
  length-2 array invariant checked. Tags 6/7/8/9 decode payload
  directly; tags 0/1/2/3/4/5/10 capture the remaining bytes via
  `Decoder::position()`. Unknown tags reject with explicit error.

6 focused unit tests:
- `_invalid_metadata_decodes_tag9` (no-payload).
- `_missing_tx_body_metadata_hash_decodes_tag6` (32-byte hash).
- `_missing_tx_metadata_decodes_tag7` (32-byte hash).
- `_conflicting_metadata_hash_decodes_tag8` (Mismatch payload).
- `_routes_unported_tag_to_raw_variant` (tag 1 captures raw).
- `_unknown_tag_rejects` (tag 99 rejection).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (166 lib + 4
  doctests, +6 new tests vs R597 baseline of 160)

## Remaining (A5 Phase-2.5+)

- NonEmptySet decoder for tags 1/2/3/10 (each carries a non-empty
  set of 28-byte hashes).
- NonEmpty list decoder for tag 0 (`NonEmpty (VKey Witness)`).
- Plain Set decoder for tag 5 (`Set (KeyHash Witness)`).
- `ShelleyUtxoPredFailure` decoder for tag 4 (nested sub-rule
  — its own multi-variant enum with witness/UTxO-balance-related
  predicate failures).
- Wire the typed `ShelleyUtxowPredFailure` into
  `ShelleyLedgerPredFailure::UtxowFailure(Vec<u8>)` → carry the
  typed enum instead of raw bytes.
- Mirror the predicate-failure tree for Allegra/Mary/Alonzo/Babbage/
  Conway eras.
