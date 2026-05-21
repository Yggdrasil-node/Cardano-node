---
title: "Round 693 tx-generator DumpToFile renders tx-body withdrawals (A4)"
parent: Reference
---

# Round 693 tx-generator DumpToFile renders tx-body withdrawals (A4)

Date: 2026-05-21

## Scope

Extends the tx-generator `DumpToFile` `Show (Tx)` renderer to
render the tx-body `Withdrawals` field across all six era
renderers (Shelley / Allegra / Mary / Alonzo / Babbage /
Conway), instead of rejecting any tx that carries a non-empty
withdrawal map.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Address.hs:183-196`
  (`RewardAccount` is a deprecated pattern synonym for
  `AccountAddress`; `Withdrawals` is keyed by `AccountAddress`,
  whose stock-derived record `Show` is `AccountAddress
  {aaNetworkId = …, aaId = …}`).
- `newtype Withdrawals = Withdrawals { unWithdrawals :: Map
  RewardAccount Coin }` — stock-derived record `Show`.

## Changes

- Added `show_withdrawals(Option<&BTreeMap<RewardAccount,
  u64>>)` — renders `Withdrawals {unWithdrawals = fromList
  [(<AccountAddress>,Coin <n>), …]}`, reusing
  `show_account_address_from_record` for each key. `BTreeMap`
  iteration order matches upstream `Data.Map` Show (sorted by
  key).
- All six era renderers (`show_shelley_tx_for_dump` …
  `show_conway_tx_for_dump`) drop the
  `ensure_empty_or_absent_btree(tx.body.withdrawals, …)` gate
  and render the field value.
- Removed the now-unused `ensure_empty_or_absent_btree` helper.

1 new focused unit test:
- `dumptofile_withdrawals_render` — empty/absent → empty
  `fromList`; one-entry map → the typed `AccountAddress` key +
  `Coin` value.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator` (241 lib + 5 main,
  +1 new test vs R692 baseline of 240)

## Remaining (A4)

- Other `DumpToFile` tx-body fields still gated by
  `ensure_absent` / `ensure_empty_or_absent` (certificates,
  mint, collateral, reference inputs, auxiliary data, update).
