# Round 837 - cardano-testnet Property/Assert CLI-backed stake-pool assertion

## Scope

Close the CLI-backed portion of upstream `Testnet/Property/Assert.hs`
`assertExpectedSposInLedgerState` while keeping the implementation testable and
bounded. The wrapper preserves the upstream `cardano-cli latest query
stake-pools --out-file <path>` argv shape, then reuses the R836 JSON
stake-pool set-count assertion once the query writes its output file.

This round deliberately does not start the `Testnet/Property/Run.hs`
Hedgehog-to-Rust harness or any node/KES spawning, era-genesis, DRep/SPO
runtime, or transaction runtime orchestration work.

## Upstream facts

- `assertExpectedSposInLedgerState` invokes `execCli' execConfig` with:
  `latest query stake-pools --out-file <output>`.
- The wrapper decodes the output file as a `Set PoolId`, so duplicate pool ids
  do not inflate the observed SPO count.
- Decode failures use the same `Failed to decode stake pools from ledger state`
  prefix as the R836 pure helper.
- Count mismatches continue to use the multiline `Expected number of stake
  pools not found in ledger state` message shape.

## Changes

- Added `stake_pools_query_args` to expose the exact upstream query argv shape.
- Added `assert_expected_spos_in_ledger_state_with_executor`, an injectable
  wrapper that passes through `ExecConfig`, invokes the caller-provided CLI
  executor, reads the generated JSON file, and reuses the R836 set-count core.
- Added `assert_expected_spos_in_ledger_state`, the real `cardano-cli` wrapper
  over the existing `process::run::exec_cli` helper.
- Added a focused test proving the wrapper calls the executor with the expected
  `ExecConfig` and argv, then validates duplicate-pool set semantics from the
  generated JSON file.
- Updated cardano-testnet status docs, parity matrix evidence, stale-status
  guards, and the living test baseline to R837 / 7,246 passing tests / 7,249
  listed tests.

## Validation

- Red first: `cargo test -p yggdrasil-cardano-testnet
  property_assert_stake_pool_query_wrapper --lib` failed with unresolved imports
  for `assert_expected_spos_in_ledger_state_with_executor` and
  `stake_pools_query_args`.
- Green focused wrapper check:
  `cargo test -p yggdrasil-cardano-testnet
  property_assert_stake_pool_query_wrapper --lib` passed with 1 test.
- Green focused Property/Assert set:
  `cargo test -p yggdrasil-cardano-testnet property_assert --lib` passed with
  3 tests.
- Green package check: `cargo test -p yggdrasil-cardano-testnet` passed 125
  lib tests plus 3 CLI golden tests.
- Formatting: `cargo fmt --all -- --check` exited 0 after applying rustfmt to
  the R837 test/import layout.
- Focused validators:
  `python scripts/check-stale-placement.py --self-test`,
  `python scripts/check-stale-placement.py`,
  `python scripts/check-doc-status-headers.py --self-test`,
  `python scripts/check-doc-status-headers.py`,
  `python scripts/check-parity-matrix.py`,
  `python scripts/check-strict-mirror.py --fail-on-violation`, and
  `python -m py_compile scripts/check-stale-placement.py
  scripts/check-doc-status-headers.py scripts/check-parity-matrix.py
  .claude/scripts/filetree.py` exited 0.
- Filetree metadata was accepted with `python .claude/scripts/filetree.py
  accept-current`; `python .claude/scripts/filetree.py check` reported all
  non-exempt entries match accepted metadata.
- Broad Rust gates: `cargo check-all`, `cargo lint`, and `cargo test-all`
  exited 0.
- Green inventory: `cargo test-all -- --list | Select-String -Pattern
  ': test$' | Measure-Object` returned `7249`.

## Remaining risk

The `cardano` and `create-env` subcommands still return the structured
deferral until node/KES spawning and supervision, era-genesis builders,
DRep/SPO runtime workflows, transaction runtime/query orchestration, and the
remaining Process/Property harnesses are ported.
