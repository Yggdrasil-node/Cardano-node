---
title: "Round 819 cardano-testnet genesis-options parser"
parent: Reference
---

# Round 819 cardano-testnet genesis-options parser

Date: 2026-05-22

## Scope

Continues the cardano-testnet era-aware CLI-parser arc — slice 2:
the `GenesisOptions` flag parser plus the value-flag parsing
infrastructure.

## What shipped

`crates/tools/cardano-testnet/src/parser.rs`:

- `flag_with_value` — looks up the value following a `--flag`,
  returning `Ok(None)` when absent, `Ok(Some(value))` when present,
  and `MissingFlagValue` when a flag has no following value.
- `flag_or_default` — a generic `--flag value` parser falling back to
  a default when the flag is absent.
- `parse_genesis_options` — mirror of upstream `pGenesisOptions`
  (`Parsers/Cardano.hs`): parses `--testnet-magic`, `--epoch-length`,
  `--slot-length`, and `--active-slots-coeff` into a `GenesisOptions`,
  each defaulting to `GenesisOptions::default()`.
- `ParseError` gains `InvalidFlagValue` and `MissingFlagValue`.

The module-docstring carve-out claiming the era-aware payload is
blocked on yggdrasil-ledger's era surface was stale (resolved by the
R783-R786 era-type ports) and is corrected.

4 unit tests cover the all-defaults case, each flag parsed, a bad
value rejected, and a flag missing its value.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 82 lib (+4 vs R818's
  78), all green.

## Remaining (cardano-testnet CLI parser)

`pTestnetNodeOptions`, `pNumDReps`, `pMaxLovelaceSupply`,
`pOnChainParams`, `pCreationOptions`, `pFromEnv`, and the
`optsTestnet` / `optsCreateTestnet` composition.
