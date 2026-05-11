---
title: 'R496: Block::emit_traces body + TraceLedgerProcessing trace-event wiring'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-496-emit-traces-body/
---

# R496 — `Block::emit_traces` body + R488 trace-event wiring

**Date:** 2026-05-11
**Predecessor:** R495 (`Tx::decode_fee` + `Tx::decode_ttl`).
**Scope:** single-round — replace R476's empty `emit_traces`
placeholder with a block-iteration-derived body, then wire the
output into R488's `TraceLedgerProcessing` outcome.

## Slice scope

R476 shipped `impl HasAnalysis for Block` with an `emit_traces`
that returned `Vec::new()` — a placeholder pending ledger-state
apply-loop. R488 documented this as a carve-out (the
`TraceLedgerProcessing` outcome carries Ok/Err apply outcomes
but no per-block trace-event strings).

R496 closes the gap by emitting block-iteration-derivable trace
strings:

```rust
fn emit_traces(with_state: &WithLedgerState<Self, ...>) -> Vec<String> {
    let blk = &with_state.blk;
    let mut traces = vec![
        format!("event=block_apply"),
        format!("slot={}", blk.header.slot_no.0),
        format!("block_no={}", blk.header.block_no.0),
        format!("era={:?}", blk.era),
        format!("tx_count={}", blk.transactions.len()),
    ];
    // EBB marker via byron_ebbs registry lookup.
    if crate::byron_ebbs::known_ebbs().contains_key(&blk.header.hash) {
        traces.push("ebb=true".to_string());
    }
    // Origin-successor marker.
    if blk.header.prev_hash.0 == [0u8; 32] {
        traces.push("prev=<origin>".to_string());
    }
    traces
}
```

Every block gets 5 baseline trace strings; EBB blocks get an
extra `ebb=true`; genesis-successor blocks get an extra
`prev=<origin>`. Ledger-state-derived traces (epoch boundary,
stake delta, era transitions) still need genesis-bootstrap —
documented as a separate follow-on.

## R488 outcome expansion

`AnalysisOutcome::TraceLedgerProcessing` gains a new
`emit_traces: Vec<Vec<String>>` field parallel to `traces`. The
runner now invokes `Block::emit_traces` per block via a
`WithLedgerState::new(blk.clone(), CardanoLedgerStateValues,
CardanoLedgerStateValues)` wrapper.

Stdout renderer in `lib.rs::render_outcome` extends per-block
output:

```
slot=10 block_no=100 apply=ok
  trace: event=block_apply
  trace: slot=10
  trace: block_no=100
  trace: era=Shelley
  trace: tx_count=0
slot=20 block_no=101 apply=ok
  trace: ...
trace_ledger_processing applied_ok=2 applied_err=0
```

## Tests delivered (+4 cases, 1 reshaped)

New:
- `analysis_trace_ledger_processing_emits_per_block_trace_strings_r496`
  (2-block chain; asserts 5 canonical keys per block + correct
  slot values).
- `analysis_trace_ledger_processing_emits_origin_marker_for_genesis_successor`
  (block with prev_hash=zeros gets `prev=<origin>` trace).
- `analysis_trace_ledger_processing_emits_ebb_marker_for_known_byron_ebb`
  (plant the real Byron mainnet genesis-successor hash; assert
  `ebb=true` trace).
- `block_emit_traces_returns_block_iteration_traces_r496`
  (replaces R476's `_returns_empty_pending_ledger_state_arc`;
  now asserts the 5 expected keys + values).

Reshaped: 3 existing R488 destructure-tests now mention the new
`emit_traces` field via `emit_traces: _` or `let _ = &emit_traces;`.

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,216 → 6,219
python3 scripts/check-strict-mirror.py --fail-on-violation   0 violations
python3 scripts/check-parity-matrix.py                       clean
```

## Stop point

R488's `TraceLedgerProcessing` outcome now carries real per-
block trace-event strings — closes the trace-content gap
documented at R488 closure. Combined with R494+R495
(`decode_inputs`/`decode_fee`/`decode_ttl`), the
`ReproMempoolAndForge` mempool entries carry 7/8 real fields.

The R475-R496 sequence has shipped a fully-functional
`db-analyser` binary at high forensic fidelity across 22
rounds. Remaining gaps are deep multi-round commitments
(genesis-bootstrap CLI flags + protocol-params hydration +
ledger-state-aware revalidation) not blocking dispatch or
operational use.
