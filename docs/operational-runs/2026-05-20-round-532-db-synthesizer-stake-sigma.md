# R532 db-synthesizer Stake Sigma

Date: 2026-05-20

## Scope

Closed db-synthesizer Phase 4 R3c-5 by replacing the temporary
full-stake Praos lottery with ledger-view stake snapshots.

## Changes

- `forging.rs` now carries `StakeSnapshots` in `ForgeState`, computes
  each forger's pool id from the cold issuer key hash, and passes the
  pool's `StakeSnapshots.set` relative stake into `check_should_forge`.
- Praos replay and forward forging apply ledger epoch boundaries via
  `apply_epoch_boundary`, rotating stake snapshots as synthetic slots
  cross epoch boundaries.
- `run.rs` seeds the initial forecast stake snapshot from Shelley
  genesis `staking.pools`, `staking.stake`, and `initialFunds`.
- `yggdrasil-node-genesis` now parses Shelley genesis `staking.pools`
  into `PoolParams`; `LedgerState` activates those genesis pools with
  genesis stake on the first Shelley-family block.
- db-synthesizer integration fixtures now generate a staked genesis
  whose pool key and VRF hash match the bulk credentials under test.

## Verification

- `cargo check -p yggdrasil-db-synthesizer`
- `cargo test -p yggdrasil-db-synthesizer`

## Remaining Gate

Operator swap-in still requires the upstream ChainDB byte-equivalence
soak against the Haskell `db-synthesizer` binary.
