---
title: "Round 604 Network enum + NonEmptySet AccountAddress decoder (A5 Phase-2.5)"
parent: Reference
---

# Round 604 Network enum + NonEmptySet AccountAddress decoder (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `Network` enum and `NonEmptySetAccountAddress` carrier
+ decoder, wires `ShelleyUtxoPredFailure::WrongNetworkWithdrawal`
(tag 9 — 3-element CBOR envelope) to typed payload. 6/11 UTXO
variants now carry typed payloads.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/BaseTypes.hs:871-891`
  (`data Network = Testnet | Mainnet`, Word8 CBOR encoding via
  `networkToWord8`/`word8ToNetwork`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs:181-183,239-240`
  (tag 9 `WrongNetworkWithdrawal Network (NonEmptySet
  AccountAddress)` 3-element envelope).

## Changes

- Added `Network` enum (`Testnet=0`, `Mainnet=1`) with
  `from_decoder` (reads Word8, validates range) and Display
  matching upstream stock-derived constructor-name Show.
- Added `NonEmptySetAccountAddress` struct
  (`BTreeSet<yggdrasil_ledger::RewardAccount>`) with both
  `from_cbor` and `from_decoder` entry points. Tag-258 tolerant,
  non-empty invariant enforced.
- Display: `NonEmptySet (fromList [AccountAddress {aaNetworkId =
  <Mainnet|Testnet>, aaId = <KeyHashObj|ScriptHashObj ...>},
  ...])` matching upstream stock-derived Show.
- Refactored `ShelleyUtxoPredFailure::WrongNetworkWithdrawal` from
  tuple variant `(Vec<u8>)` to struct variant `{ expected:
  Network, wrongs: NonEmptySetAccountAddress }`.
- Updated `from_cbor` dispatcher: tag 9 reads the in-progress
  decoder for `expected` then `wrongs` (3-element envelope
  required; length validation enforced).
- Updated Display: routes `WrongNetworkWithdrawal` through typed
  fields producing `WrongNetworkWithdrawal <Network>
  (<NonEmptySet>)`.
- Lint cleanup: collapsed the `5..=8 | 10` raw-variant arm to
  exclude tag 9 (now typed).

3 new focused unit tests:
- `network_from_decoder_round_trips` (Testnet/Mainnet/unknown).
- `_wrong_network_withdrawal_decodes_tag9` end-to-end with 1
  AccountAddress entry; verifies Network value, set size, and
  full Display shape.
- `_wrong_network_withdrawal_rejects_wrong_envelope_length`
  (rejects 2-element envelope).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint` (fixed a `non_snake_case` warning on a test-local
  binding)
- `cargo test -p yggdrasil-cardano-submit-api` (188 lib + 4
  doctests + 1 main, +3 new tests vs R603 baseline of 185)

## Remaining (A5 Phase-2.5+)

- Era-specific `Mismatch<Value era>` decoder for tag 5
  (Shelley=Coin, Mary+=MultiAsset).
- `NonEmpty (TxOut era)` decoder for tags 6 and 10.
- `ShelleyPpupPredFailure` decoder for tag 7.
- `NonEmptySet Addr` decoder for tag 8 (3-element envelope —
  Network + Addr).
- Wire typed `ShelleyUtxoPredFailure` into
  `ShelleyUtxowPredFailure::UtxoFailure(Vec<u8>)`.
- Wire typed `ShelleyUtxowPredFailure` into
  `ShelleyLedgerPredFailure::UtxowFailure(Vec<u8>)`.
- `ShelleyDelegsPredFailure` decoder for LEDGER tag 1.
- Mirror per-era predicate-failure tree for Allegra/Mary/Alonzo/
  Babbage/Conway eras.
