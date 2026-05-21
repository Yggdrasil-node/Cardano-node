---
title: "Round 716 db-analyser genesis-bootstrap arc close (slice 5b)"
parent: Reference
---

# Round 716 db-analyser genesis-bootstrap arc close (slice 5b)

Date: 2026-05-21

## Scope

Final slice (5b) of the db-analyser genesis-bootstrap arc. Wires
`run` to supply the genesis-seeded `LedgerState` to the analysis
runner, closing the arc R488 first deferred.

## What shipped

`crates/tools/db-analyser/src/lib.rs`:

- `run` takes a new `cardano_args: Option<&CardanoBlockArgs>`
  parameter. When `Some` (operator passed `--config`), `run` calls
  `<Block as HasProtocolInfo>::make_protocol_info` (R713) +
  `build_genesis_ledger_state` (R714) to build the genesis-seeded
  initial `LedgerState`, and threads it through `run_analysis`. When
  `None`, the analyses keep the empty-`LedgerState::new()` behavior.
- `run_main` passes the parsed `cardano_args` (R711) through to
  `run`.

`crates/tools/db-analyser/src/analysis/runner.rs`:

- `run_analysis` takes a `genesis_ledger_state: Option<LedgerState>`
  parameter and threads it into the 5 ledger-applying handler arms
  (replacing the `None` placeholders from slice 5a).

## End-to-end coverage

2 new end-to-end tests:

- `end_to_end_lib_run_with_config_threads_genesis_seeded_state` —
  the full `run(&config, Some(&cardano_args))` path (config →
  `make_protocol_info` → `build_genesis_ledger_state` →
  `run_analysis`) completes without error for a ledger-applying
  analysis.
- `end_to_end_genesis_seeded_state_changes_store_ledger_snapshot` —
  a `StoreLedgerStateAt` run seeded from a node config captures a
  snapshot that differs from the `None` run's empty-state snapshot:
  proof the genesis-seeded state genuinely reaches the handlers.

## Genesis-bootstrap arc — CODE COMPLETE (R710-R716)

`db-analyser` now accepts `--config PATH`, builds the genesis-seeded
initial `LedgerState`, and the 5 ledger-applying analyses
(`TraceLedgerProcessing`, `BenchmarkLedgerOps`, `StoreLedgerStateAt`,
`GetBlockApplicationMetrics`, `ReproMempoolAndForge`) bootstrap from
it instead of `LedgerState::new()`:

- R710 — `CardanoBlockArgs` config-args type.
- R711 — `--config` / `--threshold` parser flags (`parse_cmd_line`).
- R712 — `CardanoConfig` node-config type + serde decode.
- R713 — `HasProtocolInfo for Block` (`make_protocol_info`).
- R714 — `build_genesis_ledger_state` (bundle → `LedgerState`).
- R715 — handlers accept an injectable initial state.
- R716 — `run` supplies the genesis-seeded state.

A full operator rehearsal (`db-analyser --config <preview-config>
--db <synthesized-chain>`) remains an operator-side integration
exercise; the code path is complete and the R713/R714/R716 tests
exercise it against the real vendored `configuration/preview/`
bundle and synthetic ChainDbs.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-db-analyser` — 218 lib + 22 end-to-end
  (+2 vs R715's 20) + 2 golden, all green.
