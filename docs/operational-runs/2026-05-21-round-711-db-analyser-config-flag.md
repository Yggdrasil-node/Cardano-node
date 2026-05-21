---
title: "Round 711 db-analyser --config parser flag (genesis-bootstrap arc, slice 2)"
parent: Reference
---

# Round 711 db-analyser --config parser flag (genesis-bootstrap arc, slice 2)

Date: 2026-05-21

## Scope

Second slice of the db-analyser genesis-bootstrap arc. Wires the
`--config PATH` and `--threshold FLOAT` CLI flags into the parser,
producing the `CardanoBlockArgs` (R710) the rest of the arc consumes.

## What shipped

`crates/tools/db-analyser/src/parser.rs`:

- `parse_cmd_line` — new public entry mirroring upstream
  `parseCmdLine = (,) <$> parseDBAnalyserConfig <*> parseCardanoArgs`
  (`.reference-haskell-cardano-node/.../app/DBAnalyser/Parsers.hs`).
  Returns `(DBAnalyserConfig, Option<CardanoBlockArgs>)`.
- `--config PATH` and `--threshold FLOAT` flags + `RawArgs` fields +
  a `parse_f64` value helper.
- `promote_cardano` — builds the `Option<CardanoBlockArgs>` half;
  `--threshold` without `--config` is rejected with the new
  `ParseError::ThresholdWithoutConfig` variant.
- `parse_args` retained as a thin convenience over `parse_cmd_line`
  that drops the `CardanoBlockArgs` half — every existing parser /
  end-to-end / golden test stays green untouched.
- Un-did the explicit `parseCardanoArgs` / `CardanoBlockArgs`
  "NOT ported, by design" carve-out in the module docstring.

`crates/tools/db-analyser/src/lib.rs`:

- `run_main` switched from `parse_args` to `parse_cmd_line`; the
  parsed `CardanoBlockArgs` is bound (`_cardano_args`) pending the
  slice-3/4 genesis-seeded ledger bootstrap that consumes it.

6 new parser unit tests (config present/absent, config+threshold,
`--threshold` alone rejected, invalid-float rejected, `parse_args`
drops the cardano half).

## Upstream divergence (recorded)

Upstream `parseConfigFile` is a **required** `strOption` — upstream
`db-analyser` always needs protocol info because `CardanoBlock` is
era-tagged and cannot be decoded without it. yggdrasil's `db-analyser`
operates on the unified `yggdrasil_ledger::Block`, whose
block-iteration analyses decode without protocol info (the R475-R481
carve-out), so `--config` is kept **optional**. When omitted,
`cardano_block_args` is `None` and only the genesis-free analyses are
meaningful. Per-analysis enforcement of "config required for the 6
ledger-applying analyses" is a slice-4 concern.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-db-analyser` — 201 lib (+6 vs R710's
  195) + 20 end-to-end + 2 golden, all green.

## Remaining (db-analyser genesis-bootstrap arc)

- Slice 3 — `CardanoConfig` mirror + config→genesis-bundle loading
  (`HasProtocolInfo for yggdrasil_ledger::Block`). Warrants a
  `parity-plan` (genesis-config parsing).
- Slice 4 — load the genesis-seeded `LedgerState` in `run`.
- Slice 5 — thread it into the analysis runner.
