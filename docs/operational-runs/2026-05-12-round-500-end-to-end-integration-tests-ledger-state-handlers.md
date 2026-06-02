---
title: 'R500: End-to-end integration tests for R488-R493 ledger-state-dependent handlers'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-12-round-500-end-to-end-integration-tests-ledger-state-handlers/
---

# R500 — End-to-end integration tests for ledger-state-dependent handlers

**Date:** 2026-05-12
**Scope:** integration-test enrichment.
**Predecessor:** R499 (parity-matrix refresh).

## Slice scope

R481 shipped 4 integration tests at
`crates/tools/db-analyser/tests/end_to_end_chain_walk.rs`
covering the block-iteration-only handlers (CountBlocks,
ShowSlotBlockNo, OnlyValidation, lib::run rendering, empty
chain). R488/R489/R490/R491/R493 shipped 5 ledger-state-
dependent handlers via the `LedgerState::apply_block` seam, but
none had end-to-end FileImmutable integration tests yet — they
only had in-memory unit tests in `analysis/runner.rs::tests`.

R500 closes that gap. 5 new integration tests exercise the
production call path (FileImmutable open → suffix_after →
run_analysis dispatch → outcome assertion) for each
ledger-state-dependent handler:

| Test | Coverage |
|------|----------|
| `end_to_end_trace_ledger_processing_via_file_immutable` | R488 + R496: 2 Byron blocks; asserts `applied_ok=2`, `traces.len()=2`, `emit_traces.len()=2`, each emit_traces vec includes "event=block_apply" + "era=Byron" |
| `end_to_end_benchmark_ledger_ops_via_file_immutable` | R489: 3-block chain; asserts SlotDataPoint records (`total_time == mut_block_apply` invariant; slot_gap=0/10/10 between consecutive blocks) |
| `end_to_end_store_ledger_state_at_via_file_immutable` | R491: 3-block chain, target_slot=20; asserts `reached_slot=Some(20)`, snapshot_bytes non-empty, all 3 blocks applied |
| `end_to_end_repro_mempool_and_forge_via_file_immutable` | R493: 1-block Byron chain (no transactions); asserts insert_count=0, forge_count=0, applied_ok=1 |
| `end_to_end_get_block_application_metrics_via_file_immutable` | R490: 2-block chain with `every_n_blocks=1`; asserts row-per-block + the 4 R476 columns (slot/block_no/era/tx_count) present |

## Why integration tests matter here

The in-memory unit tests in `analysis/runner.rs` exercise the
handler functions directly with `Vec<Block>` input. The
integration tests exercise the **full production call path** —
including `FileImmutable::open` (which does CBOR-decode every
block on open), `suffix_after(&Point::Origin)` (which walks the
chain in chunk order), and the runner's `Block` iteration.

This catches gap-classes the unit tests miss:
- CBOR round-trip preserving Block fields the handler reads.
- `FileImmutable` open + reopen seeing the same blocks the test
  wrote.
- Chain-order preservation through the storage layer.

## Tests delivered (+5 cases)

Integration test file:
`crates/tools/db-analyser/tests/end_to_end_chain_walk.rs` now
ships 11 tests (was 6 at R488 + R491 + R493 + R485 sequence).
Each new test uses Byron-era blocks for clean apply-loop
semantics (no genesis-state-dependent UTxO lookups required;
matches the R488 forensic semantic).

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,224 → 6,229
python3 dev/test/check-strict-mirror.py --fail-on-violation   0 violations
python3 dev/test/check-parity-matrix.py                       clean
```

## Stop point — db-analyser surface fully tested at end-to-end

After R500, every shipped db-analyser handler (12 total: 7
block-iteration-only + 5 ledger-state-dependent) has at least
one end-to-end FileImmutable integration test. The R475-R500
sequence has shipped a fully-functional `db-analyser` binary at
high forensic fidelity with comprehensive test coverage across
26 rounds.
