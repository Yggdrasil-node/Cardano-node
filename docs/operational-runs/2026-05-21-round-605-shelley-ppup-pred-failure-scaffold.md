---
title: "Round 605 ShelleyPpupPredFailure scaffold + wire UTXO tag 7 (A5 Phase-2.5)"
parent: Reference
---

# Round 605 ShelleyPpupPredFailure scaffold + wire UTXO tag 7 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Opens the nested `ShelleyPpupPredFailure` scaffold (PPUP sub-rule
under `ShelleyUtxoPredFailure::UpdateFailure`, UTXO tag 7) and
wires the parent variant to the typed enum. After R605, 7/11 UTXO
variants carry typed payloads. Per-variant PPUP payload decoders
(Mismatch<SetKeyHash>, 3-Word PPUpdateWrongEpoch, ProtVer 2-array)
ship in R606+.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Ppup.hs:86-108`
  (`data ShelleyPpupPredFailure era` 3-variant ADT with stock-
  derived Show).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Ppup.hs:133-145`
  (`encCBOR` / `decCBOR` via upstream's `Sum` constructor — CBOR
  list with Word8 tag at index 0).

## Changes

- Added `ShelleyPpupPredFailure` 3-variant enum:
  - Tag 0 `NonGenesisUpdatePPUP(Vec<u8>)` — pending typed
    `Mismatch<SetKeyHash>` decoder.
  - Tag 1 `PPUpdateWrongEpoch(Vec<u8>)` — pending typed
    `(EpochNo, EpochNo, VotingPeriod)` decoder.
  - Tag 2 `PVCannotFollowPPUP(Vec<u8>)` — pending typed `ProtVer`
    decoder.
- Helpers: `tag()`, `constructor()`, `from_cbor` (outer-envelope
  walker reading tag + capturing remaining bytes verbatim).
- Display: `<Constructor> <raw-cbor N bytes>` matching the
  scaffold pattern from R598.
- Refactored `ShelleyUtxoPredFailure::UpdateFailure(Vec<u8>)` →
  `UpdateFailure(ShelleyPpupPredFailure)`. UTXO `from_cbor`
  dispatcher routes tag 7 through `ShelleyPpupPredFailure::from_cbor`;
  Display emits `UpdateFailure (<PpupVariant> ...)`.

4 focused unit tests:
- `_tag_dispatch` exercises all 3 PPUP tags via constructed CBOR.
- `_display_marks_raw_cbor` pins the raw-cbor marker shape.
- `_unknown_tag_rejects` validates the tag-42 rejection path.
- `_utxo_update_failure_routes_to_typed_ppup` end-to-end: UTXO
  tag 7 with inner PPUP tag 2 decodes to typed nested variant.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (192 lib + 4
  doctests + 1 main, +4 new tests vs R604 baseline of 188)

## Remaining (A5 Phase-2.5+)

- PPUP per-variant typed decoders:
  - Tag 0: `Mismatch<Set<KeyHash>>` decoder (RelSubset relation).
  - Tag 1: 3-Word envelope (Word64, Word64, Word8 VotingPeriod
    {0=VoteForThisEpoch, 1=VoteForNextEpoch}).
  - Tag 2: `ProtVer` (2-element record [Word major, Word minor]).
- UTXO tags still raw: 5 (era-specific Value), 6 (NonEmpty
  TxOut), 8 (Network+Addr), 10 (NonEmpty TxOut).
- Wire typed `ShelleyUtxoPredFailure` into
  `ShelleyUtxowPredFailure::UtxoFailure(Vec<u8>)`.
- Wire typed `ShelleyUtxowPredFailure` into
  `ShelleyLedgerPredFailure::UtxowFailure(Vec<u8>)`.
- `ShelleyDelegsPredFailure` decoder for LEDGER tag 1.
- Mirror per-era predicate-failure tree for Allegra/Mary/Alonzo/
  Babbage/Conway eras.
