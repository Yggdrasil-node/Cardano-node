---
title: "Round 818 cardano-testnet runtime-options parser"
parent: Reference
---

# Round 818 cardano-testnet runtime-options parser

Date: 2026-05-22

## Scope

Opens the cardano-testnet era-aware CLI-parser arc — unblocked by
the R783-R786 type ports (`CardanoEra` / `ShelleyBasedEra`, the era
option records). Slice 1: the `TestnetRuntimeOptions` flag parser.

## What shipped

`crates/tools/cardano-testnet/src/parser.rs`:

- `parse_runtime_options` — mirror of upstream `pRuntimeOptions`
  (`Parsers/Cardano.hs`): parses the `--enable-new-epoch-state-logging`
  switch, the `--enable-grpc` flag (`RpcEnabled` / `RpcDisabled`), and
  the `--use-kes-agent` flag (`UseKesSocket` / `UseKesKeyFile`) from a
  `cardano` / `create-env` argument list into a `TestnetRuntimeOptions`.

The era-aware CLI parser was previously a carve-out ("depends on
`Cardano.Api` era machinery"); R783-R786 ported `CardanoEra` /
`ShelleyBasedEra` and the option records, so the parser is now a
genuine bounded arc — `Parsers/Cardano.hs`'s option parsers feeding
the `CardanoTestnetCliOptions` record.

2 unit tests cover the all-defaults and all-flags-set cases.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 78 lib (+2 vs R785's
  76), all green.

## Remaining (cardano-testnet CLI parser)

The remaining `Parsers/Cardano.hs` option parsers — `pGenesisOptions`,
`pTestnetNodeOptions`, `pCreationOptions`, `pFromEnv`, `pOnChainParams`
— and the `optsTestnet` / `optsCreateTestnet` composition that builds
`CardanoTestnetCliOptions` / `CardanoTestnetCreateEnvOptions`.
