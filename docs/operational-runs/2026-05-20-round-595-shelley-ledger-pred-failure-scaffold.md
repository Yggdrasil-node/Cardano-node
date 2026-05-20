---
title: "Round 595 Shelley LEDGER predicate-failure scaffold (A5 Phase-2.5)"
parent: Reference
---

# Round 595 Shelley LEDGER predicate-failure scaffold (A5 Phase-2.5)

Date: 2026-05-20

## Scope

Begins A5 Phase-2.5 by adding the typed Shelley LEDGER
predicate-failure 4-variant enum that sits inside an
`EraApplyTxError` payload. Each variant currently carries raw CBOR
bytes; per-variant typed payloads (UTXOW + DELEGS sub-rule
decoders, Withdrawals map decoder, NonEmptyMap Mismatch decoder)
land in follow-on rounds.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Ledger.hs:126-131`
  (`data ShelleyLedgerPredFailure era` 4-variant ADT).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Ledger.hs:218-248`
  (CBOR encoder/decoder — outer 2-element array with Word8 tag
  discriminator: 0=Utxow, 1=Delegs, 2=WithdrawalsMissing,
  3=IncompleteWithdrawals).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/BaseTypes.hs:826-837`
  (`Mismatch` record with custom `Show`).

## Changes

- `crates/tools/cardano-submit-api/src/types.rs` adds
  `ShelleyLedgerPredFailure` 4-variant enum:
  - `UtxowFailure(Vec<u8>)` — tag 0
  - `DelegsFailure(Vec<u8>)` — tag 1
  - `ShelleyWithdrawalsMissingAccounts(Vec<u8>)` — tag 2
  - `ShelleyIncompleteWithdrawals(Vec<u8>)` — tag 3
- Helpers: `tag()` returns Word8 discriminator, `constructor()`
  returns upstream stock-derived Show name, `raw_inner()` returns
  the raw payload bytes.
- Display impl emits `<Constructor> <raw-cbor N bytes>` marking
  the rendering as raw-cbor pending per-variant typed payloads.

Each predicate-failure variant payload is intentionally raw
because the typed expansion requires porting the upstream UTXOW +
DELEGS sub-rule predicate-failure sums (10+ variants each, with
their own nested type machinery — UTxO failures wrap the offending
TxIns, DELEGS failures wrap nested DELPL/POOL/DELEG failures).
Those typed payloads ship in follow-on R596+ rounds.

4 focused unit tests pin the tag dispatch, constructor names,
Display shape, and raw_inner round-trip.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (154 lib + 4
  doctests, +4 new tests vs R594 baseline of 150)

## Remaining (A5 Phase-2.5+)

- Typed `Withdrawals` payload decoder for tag 2 (Map AccountAddress
  Coin — yggdrasil's existing RewardAccount + Coin codecs).
- Typed `Mismatch RelEQ Coin` decoder for the tag-3 NonEmptyMap.
- `ShelleyUtxowPredFailure` 10-variant sub-rule for tag 0 + its
  CBOR decoder.
- `ShelleyDelegsPredFailure` sub-rule for tag 1 (delegates further
  into DELPL/POOL/DELEG sub-rules).
- Mirror the same predicate-failure tree for Allegra/Mary/Alonzo/
  Babbage/Conway eras (Conway adds 4+ Conway-specific
  predicate-failure variants on top of the Babbage set).
- Hook the typed decoder into
  `TxValidationErrorInCardanoMode::Display` so operators get full
  upstream-shape rendering without the raw-cbor marker.
