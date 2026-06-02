---
title: "Round 820 cardano-testnet node + on-chain-params parsers"
parent: Reference
---

# Round 820 cardano-testnet node + on-chain-params parsers

Date: 2026-05-22

## Scope

Continues the cardano-testnet era-aware CLI-parser arc — slice 3:
the testnet-node-set and on-chain-params flag parsers.

## What shipped

`crates/tools/cardano-testnet/src/parser.rs`:

- `parse_testnet_node_options` — mirror of upstream
  `pTestnetNodeOptions` (`Parsers/Cardano.hs`): `--num-pool-nodes N`
  yields `N` SPO nodes (at least one required), absent yields the
  default one-SPO / two-relay node set.
- `parse_on_chain_params` — mirror of upstream `pOnChainParams`:
  `--params-file FILEPATH` yields `OnChainParamsFile`,
  `--params-mainnet` yields `OnChainParamsMainnet`, absent both
  yields `DefaultParams`.

4 unit tests cover the default node set, the count case, the
zero-rejection, and the three on-chain-params branches.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 86 lib (+4 vs R819's
  82), all green.

## Remaining (cardano-testnet CLI parser)

`parse_creation_options` (composing `parse_testnet_node_options`,
the `--num-dreps` / `--max-lovelace-supply` field flags,
`parse_genesis_options`, `parse_on_chain_params`, and the default
era), `parse_from_env`, and the top-level `opts_testnet` /
`opts_create_testnet` composition producing `CardanoTestnetCliOptions`
/ `CardanoTestnetCreateEnvOptions`.
