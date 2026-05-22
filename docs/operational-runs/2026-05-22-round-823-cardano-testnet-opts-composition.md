---
title: "Round 823 cardano-testnet opts composition"
parent: Reference
---

# Round 823 cardano-testnet opts composition

Date: 2026-05-22

## Scope

Continues the cardano-testnet era-aware CLI-parser arc — slice 6:
the top-level `opts_testnet` / `opts_create_testnet` composition.

## What shipped

`crates/tools/cardano-testnet/src/parser.rs`:

- `opts_testnet` — mirror of upstream `optsTestnet`
  (`Parsers/Cardano.hs`): the presence of `--node-env` selects the
  start-from-environment mode (`StartFromEnv`), otherwise a new
  environment is created (`NoUserProvidedEnv`, with an optional
  `--output-dir`); either mode carries the runtime options. Produces
  a `CardanoTestnetCliOptions`.
- `opts_create_testnet` — mirror of upstream `optsCreateTestnet`:
  the creation options plus the required `--output` directory.
  Produces a `CardanoTestnetCreateEnvOptions`.

3 unit tests cover the new-env default mode, the from-env mode, and
the `--output`-required behaviour.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-cardano-testnet` — 93 lib (+3 vs R822's
  90), all green.

## Remaining (cardano-testnet CLI parser)

The typed `CardanoTestnetCliOptions` / `CardanoTestnetCreateEnvOptions`
are now produced by `opts_testnet` / `opts_create_testnet`. The
final step is wiring `parse_args`' `Command::Cardano` /
`Command::CreateEnv` to carry the typed records instead of the
opaque `PassthroughArgs`.
