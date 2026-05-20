---
title: "Round 606 ShelleyPpupPredFailure per-variant typed decoders (A5 Phase-2.5)"
parent: Reference
---

# Round 606 ShelleyPpupPredFailure per-variant typed decoders (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Wires per-variant typed payload decoders for all 3
`ShelleyPpupPredFailure` variants on top of R605's scaffold.
**All 3 PPUP variants now carry typed payloads.** Closes the
PPUP sub-rule layer of the Shelley predicate-failure tree.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Ppup.hs:68-84,133-145`
  (`VotingPeriod` Word8 encoding, `ShelleyPpupPredFailure`
  Sum encoder/decoder).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/BaseTypes.hs:207-216`
  (`ProtVer` record + CBORGroup 2-tuple encoding).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/BaseTypes.hs:826-869`
  (`Mismatch` payload encoded as 2-element CBOR array).

## Changes

- Added `ProtVer { major, minor }` newtype with `from_decoder`
  reading a 2-element CBOR array. Display matches upstream
  stock-derived record Show:
  `ProtVer {pvMajor = <n>, pvMinor = <n>}`.
- Added `VotingPeriod` enum (`VoteForThisEpoch=0`,
  `VoteForNextEpoch=1`) with `from_decoder` (Word8 +
  range validation) and Display matching upstream stock-derived
  constructor-name Show.
- Refactored `SetKeyHash::from_cbor` to delegate through new
  `SetKeyHash::from_decoder` (used by both the parent rejection
  list and the PPUP Mismatch decoder).
- Refactored `ShelleyPpupPredFailure` variants:
  - `NonGenesisUpdatePPUP(Vec<u8>)` →
    `NonGenesisUpdatePPUP(Mismatch<SetKeyHash>)` with the typed
    Mismatch carrying `RelSubset` relation and supplied/expected
    SetKeyHash sets.
  - `PPUpdateWrongEpoch(Vec<u8>)` → struct variant
    `PPUpdateWrongEpoch { current: u64, proposed: u64, period:
    VotingPeriod }`.
  - `PVCannotFollowPPUP(Vec<u8>)` → `PVCannotFollowPPUP(ProtVer)`.
- Updated `ShelleyPpupPredFailure::from_cbor` dispatcher:
  - Tag 0: 2-element envelope, then inner Mismatch 2-element
    array, then two `SetKeyHash::from_decoder` calls.
  - Tag 1: 4-element envelope `[1, current, proposed, period]`.
  - Tag 2: 2-element envelope `[2, ProtVer]`.
- Updated `ShelleyPpupPredFailure::Display`:
  - Tag 0: `NonGenesisUpdatePPUP (Mismatch (RelSubset)
    {supplied: fromList [...], expected: fromList [...]})`.
  - Tag 1: `PPUpdateWrongEpoch <current> <proposed> <VotingPeriod>`.
  - Tag 2: `PVCannotFollowPPUP (ProtVer {pvMajor = <n>, pvMinor
    = <n>})`.

3 new tests + 1 updated:
- `_pv_cannot_follow_decodes_tag2` end-to-end (replaces the
  R605 raw-display test).
- `_pp_update_wrong_epoch_decodes_tag1` (3-Word + VotingPeriod
  round-trip).
- `_non_genesis_update_decodes_tag0` (full Mismatch<SetKeyHash>
  round-trip).
- Updated `_utxo_update_failure_routes_to_typed_ppup` to assert
  the typed inner ProtVer and the full UTXO → PPUP → ProtVer
  Display shape.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (194 lib + 4
  doctests + 1 main, +2 net new tests vs R605 baseline of 192 —
  added 3, replaced 1)

## Remaining (A5 Phase-2.5+)

- UTXO raw tags pending: 5 (era-specific Value), 6 / 10 (NonEmpty
  TxOut), 8 (Network + Addr).
- Wire typed `ShelleyUtxoPredFailure` into
  `ShelleyUtxowPredFailure::UtxoFailure(Vec<u8>)`.
- Wire typed `ShelleyUtxowPredFailure` into
  `ShelleyLedgerPredFailure::UtxowFailure(Vec<u8>)`.
- `ShelleyDelegsPredFailure` decoder for LEDGER tag 1.
- Mirror per-era predicate-failure tree for Allegra/Mary/Alonzo/
  Babbage/Conway eras.
