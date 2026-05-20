---
title: "Round 608 ValueNotConservedUTxO typed Coin Mismatch (A5 Phase-2.5)"
parent: Reference
---

# Round 608 ValueNotConservedUTxO typed Coin Mismatch (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Wires `ShelleyUtxoPredFailure::ValueNotConservedUTxO` (tag 5) to
typed `Mismatch<u64>` with `RelEQ` relation. For the Shelley era
(this enum's scope) Value = Coin = Word64, so the payload reuses
the R602 `decode_mismatch_u64` helper. Mary+ multi-asset Value
predicate failures live under their own era-specific
predicate-failure tree (not this enum).

After R608, 9/11 UTXO variants carry typed payloads. Only tags 6
and 10 (NonEmpty TxOut, era-specific) remain raw.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs:176-177,235`
  (tag 5 `ValueNotConservedUTxO (Mismatch RelEQ (Value era))`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Tx.hs`
  (Shelley-era `type Value ShelleyEra = Coin`).

## Changes

- Refactored `ShelleyUtxoPredFailure::ValueNotConservedUTxO(Vec<u8>)`
  → `ValueNotConservedUTxO(Mismatch<u64>)`.
- `from_cbor` dispatcher: tag 5 reuses `decode_mismatch_u64` with
  `RelEQ` relation (mirrors R602's `FeeTooSmallUTxO` pattern).
- Display routes Mismatch payload through `CoinShow` for Quiet-Show
  output:
  `ValueNotConservedUTxO (Mismatch (RelEQ) {supplied: Coin <n>,
  expected: Coin <n>})`.

1 new + 1 replaced test:
- `_value_not_conserved_decodes_tag5` end-to-end typed decode.
- Replaced R603's `_routes_tag5_to_raw_variant` with the tag-6
  equivalent (`OutputTooSmallUTxO` — still raw pending NonEmpty
  TxOut decoder).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (198 lib + 4
  doctests + 1 main, +1 net new test vs R607 baseline of 197 —
  added 2, replaced 1)

## Remaining (A5 Phase-2.5+)

- UTXO raw tags pending: 6 (`OutputTooSmallUTxO`) and 10
  (`OutputBootAddrAttrsTooBig`) — both `NonEmpty (TxOut era)`,
  era-specific.
- Full typed `Addr` Show parse (Shelley vs Bootstrap split,
  PaymentCredential + StakeReference).
- Wire typed `ShelleyUtxoPredFailure` into
  `ShelleyUtxowPredFailure::UtxoFailure(Vec<u8>)`.
- Wire typed `ShelleyUtxowPredFailure` into
  `ShelleyLedgerPredFailure::UtxowFailure(Vec<u8>)`.
- `ShelleyDelegsPredFailure` decoder for LEDGER tag 1.
- Mirror per-era predicate-failure tree for Allegra/Mary/Alonzo/
  Babbage/Conway eras.
