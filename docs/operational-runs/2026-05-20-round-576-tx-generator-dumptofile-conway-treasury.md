---
title: "Round 576 tx-generator DumpToFile Conway treasury fields"
parent: Reference
---

# Round 576 tx-generator DumpToFile Conway treasury fields

Date: 2026-05-20

## Scope

This round lifts the scalar Conway-governance boundaries in
`show_conway_tx_for_dump`: `ctbrTreasuryDonation` and
`ctbrCurrentTreasuryValue`. Previously, non-zero
`ctbrTreasuryDonation` and `Some` `ctbrCurrentTreasuryValue` failed
`SubmitMode::DumpToFile` with typed `TxGenError`s. After this round
both render via the upstream-shaped `Coin <n>` /
`SJust (Coin <n>)` Show forms.

The richer map types `ctbrVotingProcedures` and
`ctbrProposalProcedures` remain on explicit `TxGenError` boundary
pending dedicated rounds — they wrap deeply nested ADTs (`Voter`,
`GovActionId`, `VotingProcedure`, `Vote`, `Anchor`,
`ProposalProcedure`, `GovAction` with 7+ variants).

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Coin.hs:92-103`
  (`newtype Coin = Coin { unCoin :: Integer } deriving (Show) via Quiet Coin`).
- `.reference-haskell-cardano-node/deps/cardano-base/cardano-strict-containers/src/Data/Maybe/Strict.hs:49-58`
  (`data StrictMaybe a = SNothing | SJust !a deriving Show`).

## Changes

- Replaced the two rejection branches in `show_conway_tx_for_dump`
  with positive rendering paths.
- Added `show_coin(coin: u64) -> String` mirroring upstream `Show
  Coin` (Quiet suppresses the record syntax around `unCoin`, leaving
  just `Coin <n>`).
- Added `show_strict_maybe_coin(value: Option<u64>) -> String`
  mirroring stock-derived `Show (StrictMaybe Coin)`: `SNothing` or
  `SJust (Coin <n>)`. The inner `Coin` is parenthesized because
  `SJust` shows its argument at showsPrec 11 and stock-derived
  single-arg constructor Show wraps at `p > 10`.
- Added 1 focused unit test `dumptofile_show_coin_helpers` covering
  `Coin 0`, large coins, `SNothing`, `SJust (Coin 0)`, and a generic
  non-zero `SJust (Coin <n>)`.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile` (28 tests, +1
  from R575)
- `cargo test -p yggdrasil-tx-generator` (211 lib tests + 5
  CLI/golden, +1 from R575 baseline)

## Remaining

- Render `ctbrVotingProcedures` map entries (`Voter` keys,
  `VotingProcedure` values with `Vote` / `Anchor`).
- Render `ctbrProposalProcedures` OSet entries (deposit, reward
  account, `GovAction` with 7+ variants, anchor).
- Render native reference scripts (`Script::Native`) — needs the
  Timelock Show port.
- Render native scripts and bootstrap witnesses inside the witness
  set.
- Full Haskell `Show (ByteString)` mnemonic-escape coverage for byte
  parity.
- Capture upstream-binary comparison evidence once a runnable
  upstream binary environment is available.
