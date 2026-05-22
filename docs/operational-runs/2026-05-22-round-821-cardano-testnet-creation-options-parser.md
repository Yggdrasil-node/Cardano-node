---
title: "Round 821 cardano-testnet creation-options parser"
parent: Reference
---

# Round 821 cardano-testnet creation-options parser

Date: 2026-05-22

## Scope

Continues the cardano-testnet era-aware CLI-parser arc — slice 4:
`parse_creation_options`, the composition that assembles a
`TestnetCreationOptions`.

## What shipped

`crates/tools/cardano-testnet/src/parser.rs`:

- `parse_creation_options` — mirror of upstream `pCreationOptions`
  (`Parsers/Cardano.hs`): composes `parse_testnet_node_options`, the
  `--max-lovelace-supply` and `--num-dreps` field flags,
  `parse_genesis_options`, and `parse_on_chain_params` into a
  `TestnetCreationOptions`. The era is not a CLI flag — upstream's
  `pure (AnyShelleyBasedEra defaultEra)` maps to the `Default`'s
  `creation_era` (Conway).

2 unit tests cover the all-defaults case (equals
`TestnetCreationOptions::default()`) and the composed-flags case.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 88 lib (+2 vs R820's
  86), all green.

## Remaining (cardano-testnet CLI parser)

`parse_from_env` (the `--node-env` / `--update-time` parser →
`TestnetEnvOptions`) and the top-level `opts_testnet` /
`opts_create_testnet` composition producing `CardanoTestnetCliOptions`
/ `CardanoTestnetCreateEnvOptions`, then wiring `parse_args`'
`Command::Cardano` / `Command::CreateEnv` to carry the typed records.
