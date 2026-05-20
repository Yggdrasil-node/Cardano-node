---
title: "Round 617 Typed DELEG decoders for tags 7/8/13/15 (MIRPot + Mismatch) (A5 Phase-2.5)"
parent: Reference
---

# Round 617 Typed DELEG decoders for tags 7/8/13/15 (MIRPot + Mismatch) (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Wires typed payload decoders for 4 more DELEG variants:
- Tag 7 `InsufficientForInstantaneousRewardsDELEG`:
  `{ pot: MirPot, mismatch: Mismatch<u64> RelLTEQ }` (Coin).
- Tag 8 `MIRCertificateTooLateinEpochDELEG`: `Mismatch<u64>`
  with `RelLT` relation (SlotNo).
- Tag 13 `InsufficientForTransferDELEG`: same shape as tag 7.
- Tag 15 `MIRNegativeTransfer`: `{ pot: MirPot, amount: u64 }`
  (Coin).

After R617, **13 of 16 DELEG variants now carry typed payloads.**
Only the 3 Credential-carrying tags (0/1/3) remain raw pending a
Credential decoder.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/TxCert.hs:315-328`
  (`data MIRPot = ReservesMIR | TreasuryMIR` + Word8 CBOR
  encoding).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/BaseTypes.hs:810-822`
  (`Relation` kind: RelEQ / RelLT / RelGT / RelLTEQ / RelGTEQ /
  RelSubset).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Deleg.hs:104-119,170-194`
  (tag 7/8/13/15 payload shapes + CBOR encoder).

## Changes

- Added `MirPot` enum mirroring upstream `data MIRPot =
  ReservesMIR | TreasuryMIR` with `from_decoder` reading the
  Word8 (0/1) and Display matching upstream stock-derived
  constructor-name Show.
- Extended `MismatchRelation` enum with the full upstream
  Relation set: added `RelLT` and `RelGT` variants alongside the
  existing RelEQ/RelLTEQ/RelGTEQ/RelSubset.
- Refactored DELEG variants 7/13 to struct shape `{pot,
  mismatch}` and variant 15 to `{pot, amount}`; variant 8 to
  tuple `(Mismatch<u64>)`.
- `from_cbor` dispatcher now decodes:
  - Tag 7: 3-element envelope `[7, pot, mismatch-array]`.
  - Tag 8: 2-element envelope `[8, mismatch-array]`.
  - Tag 13: 3-element envelope (same shape as tag 7).
  - Tag 15: 3-element envelope `[15, pot, coin]`.
  Each variant enforces exact envelope length.
- Display routes typed payloads through their typed inner
  Display (`MirPot` constructor name + `Mismatch<CoinShow>`
  wrapping for Quiet-Show Coin output; tag 8 uses bare
  `Mismatch<u64>` because SlotNo is Quiet too).

5 new tests:
- `_insufficient_instantaneous_rewards_decodes_tag7` end-to-end.
- `_mir_too_late_decodes_tag8` (Mismatch RelLT).
- `_insufficient_for_transfer_decodes_tag13`.
- `_mir_negative_transfer_decodes_tag15`.
- `mir_pot_from_decoder_round_trips` (Reserves/Treasury/unknown).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (224 lib + 4
  doctests + 1 main, +5 new tests vs R616 baseline of 219)

## Remaining (A5 Phase-2.5+)

- DELEG variants 0/1/3 (Credential Staking decoder).
- POOL variant 1 (`StakePoolRetirementWrongEpochPOOL` flattened
  3-EpochNo encoding).
- Inner per-TxOut typed parse (era-specific Shelley/Babbage).
- Full typed `Addr` Show parse (Shelley vs Bootstrap split).
- Mirror per-era predicate-failure tree for Allegra/Mary/Alonzo/
  Babbage/Conway.
