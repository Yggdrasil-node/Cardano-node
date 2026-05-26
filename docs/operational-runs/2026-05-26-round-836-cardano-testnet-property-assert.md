# Round 836 - cardano-testnet Property/Assert pure helpers

## Scope

Port the pure, projectable surface from upstream
`Testnet/Property/Assert.hs`: JSON-lines decoding, TraceNode leader/not-leader
slot extraction, deadline failure shape, stake-pool count assertion semantics,
and era-equality mismatch messages.

This round deliberately stops before the CLI-backed `execCli'` stake-pools
query wrapper and before the `Testnet/Property/Run.hs` Hedgehog-to-Rust
execution harness.

## Upstream facts

- `readJsonLines` splits lazy bytes on newline and keeps only lines that decode
  as JSON values.
- `getRelevantSlots` parses `LogEntry TraceNode` values, keeps
  `TraceNodeIsLeader` / `TraceNodeNotLeader`, and filters each list by a slot
  lower bound.
- `assertByDeadlineIOCustom` returns `Condition not met by deadline: ...` when
  the predicate stays false past the deadline.
- `assertExpectedSposInLedgerState` decodes stake pools as a `Set PoolId`,
  so duplicate pool ids do not inflate the count.
- `assertErasEqual` reports `Eras mismatch! expected: ..., received era: ...`
  for mismatched eras.

## Changes

- Added `property/assert.rs` with `read_json_lines`,
  `read_json_lines_from_slice`, `get_relevant_slots`,
  `get_relevant_slots_from_values`, `assert_by_deadline_custom`,
  `assert_expected_spos_in_ledger_state_value`, and `assert_eras_equal`.
- Wired `pub mod assert;` through `property.rs`.
- Added focused tests covering invalid-line skipping, leader/not-leader slot
  filtering, deadline error text, set-style stake-pool counting, and era
  mismatch text.
- Updated cardano-testnet status docs, parity matrix evidence, stale-status
  guards, and the living test baseline to R836 / 7,245 passing tests / 7,248
  listed tests.

## Validation

- Red first: `cargo test -p yggdrasil-cardano-testnet property_assert --lib`
  failed with `could not find assert in property`.
- Green focused implementation check:
  `cargo test -p yggdrasil-cardano-testnet property_assert --lib` passed with
  2 tests.
- Green package check: `cargo test -p yggdrasil-cardano-testnet` passed 124
  lib tests plus 3 CLI golden tests.
- Formatting: `cargo fmt --all -- --check` exited 0.
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
  ': test$' | Measure-Object` returned `7248`.

## Remaining risk

The `cardano` and `create-env` subcommands still return the structured
deferral until node/KES spawning and supervision, era-genesis builders,
DRep/SPO runtime workflows, transaction runtime/query orchestration, and the
remaining Process/Property harnesses are ported. This R836 slice stopped before
the stake-pool assertion's `cardano-cli` query wrapper; that wrapper is tracked
by the follow-on R837 evidence.
