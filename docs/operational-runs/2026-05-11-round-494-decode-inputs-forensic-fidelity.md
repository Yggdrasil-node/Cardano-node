---
title: 'R494: per-era Tx::decode_inputs + ReproMempoolAndForge fidelity bump'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-494-decode-inputs-forensic-fidelity/
---

# R494 — `Tx::decode_inputs` + R493 fidelity bump

**Date:** 2026-05-11
**Predecessor:** R493 (`ReproMempoolAndForge` handler shipped).
**Scope:** single-round forensic-fidelity hardening for R493.

## Slice scope

R493 closed the db-analyser dispatch matrix (13/13 final
verdicts) but `ReproMempoolAndForge` shipped with a
forensic-placeholder `MempoolEntry::inputs = Vec::new()` — which
meant the mempool's conflict-detection path (insert-rejects-
double-spend) was effectively disabled. R494 wires real input
decoding through a new `Tx::decode_inputs(era)` dispatcher.

Surface added:

| File | Helper | Returns |
|------|--------|---------|
| `crates/ledger/src/eras/shelley.rs` | `ShelleyTxBody::decode_inputs(&[u8])` | `Vec<ShelleyTxIn>` |
| `crates/ledger/src/eras/alonzo.rs` | `AlonzoTxBody::decode_inputs(&[u8])` | `Vec<ShelleyTxIn>` |
| `crates/ledger/src/eras/babbage.rs` | `BabbageTxBody::decode_inputs(&[u8])` | `Vec<ShelleyTxIn>` |
| `crates/ledger/src/eras/conway.rs` | `ConwayTxBody::decode_inputs(&[u8])` | `Vec<ShelleyTxIn>` |
| `crates/ledger/src/tx.rs` | `Tx::decode_inputs(&self, era: Era)` | `Result<Vec<ShelleyTxIn>, LedgerError>` |

The `Tx::decode_inputs` dispatcher mirrors `Tx::output_count`
from R475 — single match-on-era with Byron returning empty
(forensic carve-out: Byron uses `ByronTxIn`, not `ShelleyTxIn`,
and Byron txs don't participate in the Shelley-family mempool
that R493 exercises).

Empty-body short-circuit to `Ok(Vec::new())`. Malformed bodies
propagate `LedgerError::CborDecodeError` for Shelley-family +
Alonzo+ eras.

## R493 handler update

`analysis_repro_mempool_and_forge` now reads:

```rust
let inputs = tx.decode_inputs(blk.era).unwrap_or_default();
let entry = MempoolEntry {
    era: blk.era,
    tx_id: tx.id,
    fee: 0,
    body: tx.body.clone(),
    raw_tx: tx.body.clone(),
    size_bytes: tx.serialized_size(),
    ttl: yggdrasil_ledger::SlotNo(u64::MAX),
    inputs,
};
```

The `.unwrap_or_default()` on decode failure preserves R488/R489/
R490/R491/R493's forensic stance: malformed bodies don't abort
the analysis; they're silently treated as empty-inputs (matching
upstream's "best-effort forensic walk" semantics).

## Forensic fidelity matrix (post-R494)

| MempoolEntry field | R493 (initial) | R494 (current) |
|--------------------|----------------|----------------|
| `era` | real (from Block::era) | real |
| `tx_id` | real (from Tx::id) | real |
| `body` | real (from Tx::body) | real |
| `size_bytes` | real (from Tx::serialized_size) | real |
| `inputs` | empty placeholder | **real (R494; Byron empty)** |
| `fee` | 0 placeholder | 0 placeholder |
| `raw_tx` | body placeholder | body placeholder |
| `ttl` | u64::MAX placeholder | u64::MAX placeholder |

5 of 8 fields now real (was 4 of 8). Remaining 3 placeholders
(`fee`, `raw_tx`, `ttl`) are bounded follow-on items.

## Tests delivered (+6 cases)

`crates/ledger/src/tx.rs`:
- `decode_inputs_empty_body_returns_empty_vec`
- `decode_inputs_byron_carve_out_returns_empty` (Byron returns
  empty regardless of body content)
- `decode_inputs_shelley_family_dispatch` (2 inputs through
  ShelleyTxBody; Shelley/Allegra/Mary all route the same)
- `decode_inputs_alonzo_dispatch` (3 inputs through AlonzoTxBody)
- `decode_inputs_dispatch_propagates_decode_error` (Byron
  silently empty, Shelley/Conway propagate decode errors)

`crates/tools/db-analyser/src/analysis/runner.rs`:
- `analysis_repro_mempool_and_forge_rejects_conflicting_inputs_r494`
  (constructs 2 Shelley txs sharing an input (0xAA..,0);
  asserts that only 1 of 2 successful inserts — the mempool
  conflict-detection path rejects the second.
  Operationally proves the R494 wire-up works: the mempool
  rejects double-spends in the same block.)

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,201 → 6,207
python3 dev/test/check-strict-mirror.py --fail-on-violation   0 violations
python3 dev/test/check-parity-matrix.py                       clean
```

## Stop point

R494 closes one of the three documented forensic-fidelity
carve-outs from R493 (input decoding). Remaining 2 (fee
decoding + ttl decoding) are bounded per-era follow-ons that
fit a similar pattern. Dispatch matrix unaffected — still
13/13 final verdicts.
