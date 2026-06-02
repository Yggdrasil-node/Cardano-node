---
title: "Round 715 db-analyser injectable initial LedgerState (genesis-bootstrap arc, slice 5a)"
parent: Reference
---

# Round 715 db-analyser injectable initial LedgerState (genesis-bootstrap arc, slice 5a)

Date: 2026-05-21

## Scope

Sub-slice 5a of the db-analyser genesis-bootstrap arc. The 5
ledger-applying analysis handlers in `analysis/runner.rs` now accept
an injectable initial `LedgerState`.

## What shipped

`crates/tools/db-analyser/src/analysis/runner.rs`:

- The 5 handlers that run a `LedgerState::apply_block` loop —
  `analysis_trace_ledger_processing`, `analysis_benchmark_ledger_ops`,
  `analysis_store_ledger_state_at`,
  `analysis_get_block_application_metrics`,
  `analysis_repro_mempool_and_forge` — take a new
  `initial_state: Option<LedgerState>` parameter. When `Some`, the
  apply loop bootstraps from it; when `None`, it falls back to the
  existing `LedgerState::new(first_block_era)` (Byron default for
  empty input) — behavior-preserving.
- `run_analysis` passes `None` to all 5 arms for now — the plumbing
  is in place; slice 5b wires the real genesis-seeded state.

This is the runner-side half of "the 6 ledger-applying analyses
bootstrap from a genesis-seeded `LedgerState` instead of
`LedgerState::new()`". (`CheckNoThunksEvery` is the permanent
`NotApplicableToRust` carve-out and has no apply-loop handler, so 5
handlers — not 6 — actually thread the state.)

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-db-analyser` — 218 lib (+1 vs R714's
  217) + 20 end-to-end + 2 golden, all green.

The new test `analysis_store_ledger_state_at_uses_injected_initial_state`
proves the injected state is genuinely used: a block with a garbage
transaction body fails `apply_block` (leaving the injected state
unchanged), so seeding the handler with a Byron-era vs a Conway-era
`LedgerState` yields two different captured checkpoint snapshots.

## Scope boundary

`run_analysis` still passes `None` — no genesis state reaches the
handlers from a real run yet. Slice 5b changes `run` to call
`make_protocol_info` (R713) + `build_genesis_ledger_state` (R714)
when `--config` is supplied and thread the resulting `LedgerState`
through `run_analysis` into the 5 handlers.

## Remaining (db-analyser genesis-bootstrap arc)

- Slice 5b — wire `run` / `run_main` to supply the genesis-seeded
  `LedgerState`. Closes the arc's validation gate.
