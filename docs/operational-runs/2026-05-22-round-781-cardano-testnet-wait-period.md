---
title: "Round 781 cardano-testnet TestnetWaitPeriod (components/query.rs)"
parent: Reference
---

# Round 781 cardano-testnet TestnetWaitPeriod (components/query.rs)

Date: 2026-05-22

## Scope

Continues the cardano-testnet arc — opens the `components` module
with the portable type of `Testnet/Components/Query.hs`.

## What shipped

`crates/tools/cardano-testnet/src/components.rs` — new module-tree
parent for the upstream `Testnet/Components/` directory.

`crates/tools/cardano-testnet/src/components/query.rs` — new file:

- `TestnetWaitPeriod` — a wait period during a testnet run
  (`WaitForEpochs(u32)` / `WaitForBlocks(u64)` / `WaitForSlots(u64)`),
  mirror of upstream `data TestnetWaitPeriod`. Its `Display` mirrors
  the upstream `Show` instance.

The bulk of `Query.hs` is node-querying logic (`getEpochState`,
`getGovState`, `findAllUtxos`, the `wait*` loops) which runs against
a live node and lands with the testnet-harness rounds.

`lib.rs` gains `pub mod components;`.

2 unit tests cover the `Display` rendering against the upstream
`Show` and variant distinctness.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the new files).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 67 lib (+2 vs R780's
  65), all green.

## Remaining (cardano-testnet)

The clean era-free portable surface is now substantially exhausted.
What remains — the `Components/` node-querying and genesis-creation
bodies, the era-coupled `Start/Types.hs` records and `Defaults.hs`
genesis defaults, the `Start/*` era startup, and the process harness
— is era-coupled or runtime-coupled.
