---
title: 'R497: Tx::to_raw_tx_bytes for ReproMempoolAndForge raw_tx fidelity (8/8 fields real)'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-497-to-raw-tx-bytes-fidelity/
---

# R497 — `Tx::to_raw_tx_bytes` — last forensic-fidelity gap

**Date:** 2026-05-11
**Predecessor:** R496 (`Block::emit_traces` body shipped).
**Scope:** single-round — close the last `MempoolEntry`
forensic-fidelity placeholder. **8/8 fields now real.**

## Slice scope

R493 shipped `ReproMempoolAndForge` with `MempoolEntry::raw_tx
= tx.body.clone()` as a placeholder. R497 ships a proper wire-
form CBOR helper:

```rust
pub fn to_raw_tx_bytes(&self) -> Vec<u8>
```

- **Pre-Alonzo** (`is_valid: None`): 3-element CBOR array
  `[body, witnesses_or_null, aux_data_or_null]`.
- **Alonzo+** (`is_valid: Some`): 4-element CBOR array
  `[body, witnesses_or_null, is_valid_bool, aux_data_or_null]`.

`null` is CBOR primitive `0xF6` (1 byte). Array headers
inline-encoded for counts 0..=23.

The `Tx::serialized_size()` from R475-era ledger work already
matches this layout for the pre-Alonzo case. For Alonzo+ the
wire-form is 1 byte longer (the `is_valid` bool) but
`serialized_size` excludes it deliberately (matches upstream's
`toCBORForSizeComputation` which uses the 3-element form for
fee computation regardless of era).

## R493 handler update

```rust
let raw_tx = tx.to_raw_tx_bytes();
let entry = MempoolEntry {
    era: blk.era,
    tx_id: tx.id,
    fee,
    body: tx.body.clone(),
    raw_tx,                       // R497: real wire-form CBOR
    size_bytes: tx.serialized_size(),
    ttl: yggdrasil_ledger::SlotNo(ttl),
    inputs,
};
```

## Forensic-fidelity matrix (post-R497)

| MempoolEntry field | R493 | R494 | R495 | **R497** |
|--------------------|------|------|------|----------|
| `era` | real | real | real | real |
| `tx_id` | real | real | real | real |
| `body` | real | real | real | real |
| `size_bytes` | real | real | real | real |
| `inputs` | empty | real | real | real |
| `fee` | 0 | 0 | real | real |
| `ttl` | u64::MAX | u64::MAX | real | real |
| `raw_tx` | body | body | body | **real** |

**8/8 fields real.** Forensic-fidelity matrix complete.

## Tests delivered (+5 cases)

`crates/ledger/src/tx.rs`:
- `to_raw_tx_bytes_pre_alonzo_3_element_with_witnesses_and_aux`
  (byte-exact assertion: `[0x83, body..., witnesses, aux_data]`)
- `to_raw_tx_bytes_pre_alonzo_3_element_null_witnesses`
  (`[0x83, body, 0xF6, 0xF6]` when witnesses + aux are None)
- `to_raw_tx_bytes_alonzo_plus_4_element_with_is_valid_true`
  (`[0x84, body, witnesses, 0xF5, 0xF6]`)
- `to_raw_tx_bytes_alonzo_plus_4_element_with_is_valid_false`
  (`[0x84, body, 0xF6, 0xF4, 0xF6]`)
- `to_raw_tx_bytes_matches_serialized_size_length`
  (pre-Alonzo: `to_raw_tx_bytes().len() == serialized_size()`)

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,219 → 6,224
python3 scripts/check-strict-mirror.py --fail-on-violation   0 violations
python3 scripts/check-parity-matrix.py                       clean
```

## Stop point — db-analyser surface fully shipped

After R497, the db-analyser dispatch matrix carries **13/13
final verdicts** (12 shipped + 1 permanent carve-out), and the
`ReproMempoolAndForge` handler's `MempoolEntry` construction is
at **8/8 real-field fidelity**. The 4 ledger-state-dependent
analyses (`TraceLedgerProcessing`, `BenchmarkLedgerOps`,
`GetBlockApplicationMetrics`, `StoreLedgerStateAt`) all ship
via the `LedgerState::apply_block` seam (R488/R489/R490/R491).
`ReproMempoolAndForge` ships via the `yggdrasil-consensus::Mempool`
seam (R493) with R494/R495/R496/R497 fidelity hardening.

Remaining work is genuinely deep multi-round commitments
(genesis-bootstrap CLI flags + protocol-params hydration +
ledger-state-aware revalidation), not bounded autonomous-loop
rounds.

The R475-R497 sequence shipped a fully-functional `db-analyser`
binary at high forensic fidelity across **23 rounds**.
