---
title: "Round 596 cardano-submit-api typed Withdrawals decoder (A5 Phase-2.5)"
parent: Reference
---

# Round 596 cardano-submit-api typed Withdrawals decoder (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Continues A5 Phase-2.5 by adding the first typed payload decoder
on top of R595's `ShelleyLedgerPredFailure` scaffold:
`Withdrawals::from_cbor` decodes the tag-2
`ShelleyWithdrawalsMissingAccounts` payload into a typed
`BTreeMap<RewardAccount, u64>`.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Address.hs:980`
  (`newtype Withdrawals = Withdrawals {unWithdrawals :: Map
  AccountAddress Coin}`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Ledger.hs:223,243-244`
  (tag-2 CBOR: outer-array tag 2 followed by encoded
  `Withdrawals` map).

## Changes

- `crates/tools/cardano-submit-api/Cargo.toml` adds
  `yggdrasil-ledger.workspace = true` as a direct dependency
  (previously only transitive via `yggdrasil-network`). Needed for
  the `Decoder` + `RewardAccount` types.
- `crates/tools/cardano-submit-api/src/types.rs` adds
  `Withdrawals` struct + `Withdrawals::from_cbor` decoder +
  `Display` impl:
  - Decoder walks a CBOR map, decoding each `bytes(29)` key into
    a `RewardAccount` (via `RewardAccount::from_bytes`) and each
    value as an unsigned coin.
  - Map order: stored in `BTreeMap<RewardAccount, u64>` so
    iteration follows upstream `Data.Map.toAscList` byte-lex
    order.
  - Decoder errors carry context (`Withdrawals: <reason>`).
  - Display emits `Withdrawals {unWithdrawals = fromList
    [(AccountAddress {aaNetworkId = <Network>, aaId =
    <KeyHashObj/ScriptHashObj>...}, Coin <n>),...]}` matching
    upstream stock-derived `Show Withdrawals` envelope; network
    discriminates Mainnet/Testnet, credential variant either
    KeyHashObj wrapping `KeyHash {unKeyHash = "<hex>"}` or
    ScriptHashObj wrapping `ScriptHash "<hex>"`.

3 focused unit tests:
- `withdrawals_from_cbor_empty_map` — empty map → empty `entries`
  + `fromList []` Display.
- `withdrawals_from_cbor_one_entry` — single-entry map with a
  mainnet key-hash reward account + 1_000_000 coin; verifies
  decoded fields + full Display shape.
- `withdrawals_from_cbor_rejects_invalid_account_bytes` — a
  28-byte key (one byte short for a reward account) reports an
  explicit `invalid reward-account key` error.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (157 lib + 4
  doctests, +3 new tests vs R595 baseline of 154)

## Remaining (A5 Phase-2.5+)

- Wire the typed `Withdrawals` decoder into the variant payload —
  replace `ShelleyLedgerPredFailure::ShelleyWithdrawalsMissingAccounts(Vec<u8>)`
  with `ShelleyWithdrawalsMissingAccounts(Withdrawals)` and decode
  on construction.
- Typed `Mismatch RelEQ Coin` decoder + `NonEmptyMap` decoder for
  tag-3 `ShelleyIncompleteWithdrawals`.
- `ShelleyUtxowPredFailure` 10-variant sub-rule decoder for tag-0
  + nested `ShelleyUtxoPredFailure`.
- `ShelleyDelegsPredFailure` sub-rule decoder for tag-1
  (DELPL/POOL/DELEG sub-rules).
- Mirror the same predicate-failure tree for Allegra/Mary/Alonzo/
  Babbage/Conway eras (Conway adds 4+ Conway-specific
  predicate-failure variants on top of the Babbage set).
