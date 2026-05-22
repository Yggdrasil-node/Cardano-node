---
title: "Round 776 cardano-testnet era-free option records"
parent: Reference
---

# Round 776 cardano-testnet era-free option records

Date: 2026-05-22

## Scope

Continues the cardano-testnet `Testnet/Start/Types.hs` port — the
era-free option records R359 deferred (they need no era-aware ledger
surface).

## What shipped

`crates/tools/cardano-testnet/src/types.rs`:

- `TestnetRuntimeOptions` — testnet-node runtime knobs
  (`new_epoch_state_logging`, RPC, KES source), with the upstream
  `Default` (logging on, RPC off, KES from file).
- `TestnetEnvOptions` — the `--node-env` path's environment directory
  and timestamp policy.
- `GenesisOptions` — Shelley-genesis knobs (network magic, epoch
  length, slot length, active-slot coefficient), with the upstream
  `Default` (42 / 500 / 0.1 s / 0.05). `PartialEq` only — upstream
  derives `Eq` but two fields are `f64`.
- `NodeOption` (`SpoNodeOptions` / `RelayNodeOptions`) with `is_spo`
  / `is_relay` (mirror of `isSpoNodeOptions` / `isRelayNodeOptions`)
  and `cardano_default_testnet_node_options` (one SPO + two relays).

5 unit tests cover the two `Default` impls against upstream, the
node-kind predicates, the default node set, and `TestnetEnvOptions`.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 52 lib (+5 vs R775's
  47), all green.

## Remaining (cardano-testnet `Start/Types.hs`)

The era-aware option records (`CardanoTestnetCliOptions`,
`TestnetCreationOptions`, `Conf`, `NoUserProvidedEnvOptions`,
`StartFromEnvOptions`) carry `AnyShelleyBasedEra` / per-era genesis
fields gated on the yggdrasil-ledger era surface.
