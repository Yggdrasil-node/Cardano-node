---
title: "Round 782 cardano-testnet configuration constants"
parent: Reference
---

# Round 782 cardano-testnet configuration constants

Date: 2026-05-22

## Scope

Continues the cardano-testnet arc — the era-free constants of
`Testnet/Components/Configuration.hs`.

## What shipped

`crates/tools/cardano-testnet/src/components/configuration.rs` — new
file:

- `START_TIME_OFFSET_SECONDS` — seconds added to "now" for a fresh
  testnet's genesis start time, mirror of upstream
  `startTimeOffsetSeconds = if OS.isWin32 then 90 else 15` (the
  Windows / non-Windows split is reproduced with `cfg!(windows)`).
- `NUM_SEEDED_UTXO_KEYS` — `3`, mirror of upstream
  `numSeededUTxOKeys`.

`Configuration.hs` is otherwise era / IO-coupled (`createConfigJson`,
`createSPOGenesisAndFiles`, the genesis-hash helpers), gated on the
yggdrasil-ledger era surface. `components.rs` gains
`pub mod configuration;`.

2 unit tests pin both constants.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the new file).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 69 lib (+2 vs R781's
  67), all green.

## cardano-testnet — era-free portable surface complete

With this round the cardano-testnet clean, era-free portable surface
is complete (R772-R782): the option/record/path/script types across
`Start/Types.hs`, `runtime_types.rs`, `paths.rs`, `filepath.rs`,
`defaults.rs`, and `components/`. What remains is era-coupled (the
`Start/Types.hs` era-aware records, `Defaults.hs` per-era genesis,
the `Components/` node-query/genesis-creation bodies, `Start/*` era
startup) or the Hedgehog→tokio process-harness carve-out — gated on
the yggdrasil-ledger era surface and the harness rounds.
