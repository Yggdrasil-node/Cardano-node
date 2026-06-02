---
title: "Round 784 cardano-testnet TestnetCreationOptions"
parent: Reference
---

# Round 784 cardano-testnet TestnetCreationOptions

Date: 2026-05-22

## Scope

Continues the cardano-testnet arc — `TestnetCreationOptions`, the
era-aware option record unblocked by the R783 era-tag enums.

## What shipped

`crates/tools/cardano-testnet/src/types.rs`:

- `TestnetCreationOptions` — the environment-creation options
  (`creation_nodes` / `creation_era` / `creation_max_supply` /
  `creation_num_dreps` / `creation_genesis_options` /
  `creation_on_chain_params`), mirror of upstream
  `data TestnetCreationOptions`. All six fields are now portable —
  five were already ported; the sixth is the R783 `ShelleyBasedEra`.
- `Default` impl — mirror of upstream `instance Default` (default
  node set, Conway era, max supply 100_000_020_000_000, 3 DReps,
  default genesis + on-chain params).
- `creation_num_pools` / `creation_num_relays` — count the SPO /
  relay nodes, mirror of upstream `creationNumPools` /
  `creationNumRelays`.

`PartialEq` only — upstream derives `Eq` but `GenesisOptions` carries
`f64` fields.

2 unit tests cover the `Default` against upstream and the
pool/relay counts.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 74 lib (+2 vs R783's
  72), all green.

## Remaining (cardano-testnet `Start/Types.hs`)

The top-level CLI-options records that compose `TestnetCreationOptions`
— `CardanoTestnetCliOptions`, `NoUserProvidedEnvOptions`,
`StartFromEnvOptions`, `CardanoTestnetCreateEnvOptions` — land next.
