# R533 tx-generator Command Parser

Date: 2026-05-20

Area: sister-tools / `crates/tools/tx-generator`

## Scope

Started the concrete `tx-generator` A4 implementation arc by replacing
the raw passthrough parser with an upstream-shaped `Command.hs` parser
mirror. This does not claim transaction generation or submission
runtime parity yet; it closes the typed command-boundary prerequisite
for those later slices.

## Changes

- Added `src/command.rs` as the strict mirror of upstream
  `Cardano.Benchmarking.Command.hs` for the `Command` sum type and
  `commandParser` grammar.
- Parsed `json`, `json_highlevel`, `compile`, `selftest`, and
  `version`, including the `json_highlevel` `--testnet-config-dir`,
  `--nodeConfig`, `--cardano-tracer`, and sequential `-n` options.
- Updated `parser::Args` to carry typed `command::Command` instead of
  raw passthrough.
- Kept top-level `--help` / `--version` compatibility pinned to the
  captured upstream golden fixtures.
- Updated tx-generator AGENTS guidance, roadmap, parity summary, and
  parity matrix evidence.

## Verification

- `cargo test -p yggdrasil-tx-generator` (20 lib tests + 4
  CLI/golden tests)

## Remaining Gate

The next tx-generator slices are upstream setup discovery, script
compile/run behavior, generator transaction construction, and
submission client runtime parity. Operator swap-in remains blocked on
the eventual upstream binary comparison soak.
