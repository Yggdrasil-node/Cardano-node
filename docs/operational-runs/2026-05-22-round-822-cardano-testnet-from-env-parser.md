---
title: "Round 822 cardano-testnet from-env parser"
parent: Reference
---

# Round 822 cardano-testnet from-env parser

Date: 2026-05-22

## Scope

Continues the cardano-testnet era-aware CLI-parser arc — slice 5:
the genesis-timestamp policy and pre-existing-environment parsers.

## What shipped

`crates/tools/cardano-testnet/src/parser.rs`:

- `parse_update_timestamps` — mirror of upstream `pUpdateTimestamps`
  (`Parsers/Cardano.hs`): `--preserve-timestamps` →
  `DontUpdateTimestamps`; `--update-time` or neither →
  `UpdateTimestamps` (the parser default — distinct from the
  `UpdateTimestamps` type's own `Default`).
- `parse_from_env` — mirror of upstream `pFromEnv`: the required
  `--node-env FILEPATH` plus the timestamp policy → a
  `TestnetEnvOptions`.
- `ParseError` gains `MissingRequiredFlag`.

2 unit tests cover the timestamp-flag branches and the
`--node-env`-required behaviour.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 90 lib (+2 vs R821's
  88), all green.

## Remaining (cardano-testnet CLI parser)

The top-level `opts_testnet` / `opts_create_testnet` composition
(producing `CardanoTestnetCliOptions` / `CardanoTestnetCreateEnvOptions`
— the from-env vs new-env mode dispatch, the `--output-dir` /
`--output` parsers), then wiring `parse_args`' `Command::Cardano` /
`Command::CreateEnv` to carry the typed records.
