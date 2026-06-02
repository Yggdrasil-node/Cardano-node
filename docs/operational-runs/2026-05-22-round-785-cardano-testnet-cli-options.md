---
title: "Round 785 cardano-testnet CLI-options records"
parent: Reference
---

# Round 785 cardano-testnet CLI-options records

Date: 2026-05-22

## Scope

Continues the cardano-testnet arc — the top-level CLI-options records
of `Testnet/Start/Types.hs`.

## What shipped

`crates/tools/cardano-testnet/src/types.rs`:

- `NoUserProvidedEnvOptions` — options for `cardano-testnet cardano`
  with no user-provided environment (creation options, output dir,
  runtime options), mirror of upstream `data NoUserProvidedEnvOptions`.
- `StartFromEnvOptions` — options for `cardano-testnet cardano
  --node-env`, mirror of upstream `data StartFromEnvOptions`.
- `CardanoTestnetCliOptions` — the `cardano` command's options
  (`NoUserProvidedEnv` / `StartFromEnv`), mirror of upstream
  `data CardanoTestnetCliOptions`.
- `CardanoTestnetCreateEnvOptions` — the `create-env` subcommand's
  options, mirror of upstream `data CardanoTestnetCreateEnvOptions`.

All four compose the previously-ported option records
(`TestnetCreationOptions`, `TestnetRuntimeOptions`,
`TestnetEnvOptions`); `PartialEq` only, transitively via
`GenesisOptions`'s `f64` fields.

2 unit tests cover the `CardanoTestnetCliOptions` variants and the
`create-env` options.

## cardano-testnet `Start/Types.hs` operator surface complete

`types.rs` now ports the full `Start/Types.hs` operator-facing
surface — newtypes, enums, era tags, and every option record. The
remaining `Start/Types.hs` surface (`Conf` / `mkConf` directory
setup, `UserProvidedGeneses`) is IO- or era-genesis-coupled.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 76 lib (+2 vs R784's
  74), all green.
