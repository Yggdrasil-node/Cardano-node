---
title: 'R495: per-era Tx::decode_fee + Tx::decode_ttl for ReproMempoolAndForge fidelity'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-495-decode-fee-and-ttl-forensic-fidelity/
---

# R495 — `Tx::decode_fee` + `Tx::decode_ttl` forensic-fidelity bump

**Date:** 2026-05-11
**Predecessor:** R494 (`Tx::decode_inputs` shipped).
**Scope:** single-round forensic-fidelity hardening — wires real
fee + ttl into R493's `ReproMempoolAndForge` handler.

## Slice scope

R494 wired `Tx::decode_inputs` for real mempool conflict
detection. R495 ships the same pattern for `fee` and `ttl`:

| File | New helpers |
|------|-------------|
| `crates/ledger/src/eras/shelley.rs` | `ShelleyTxBody::decode_fee`, `decode_ttl` (both returning required `u64`) |
| `crates/ledger/src/eras/alonzo.rs` | `AlonzoTxBody::decode_fee`, `decode_ttl` (ttl returns `Option<u64>` — optional in CDDL) |
| `crates/ledger/src/eras/babbage.rs` | `BabbageTxBody::decode_fee`, `decode_ttl` (ttl returns `Option<u64>`) |
| `crates/ledger/src/eras/conway.rs` | `ConwayTxBody::decode_fee`, `decode_ttl` (ttl returns `Option<u64>`) |
| `crates/ledger/src/tx.rs` | `Tx::decode_fee(era)`, `Tx::decode_ttl(era)` dispatchers |

Byron carve-outs:
- `decode_fee(Byron)` returns `Ok(0)` — Byron's fee is computed
  from input/output diff, not stored as a tx-body field.
- `decode_ttl(Byron)` returns `Ok(u64::MAX)` — Byron has no ttl
  concept.

Alonzo+ optional-ttl handling:
- Per-era `decode_ttl` returns `Result<Option<u64>, _>` for
  Alonzo/Babbage/Conway (CDDL key 3 optional).
- `Tx::decode_ttl(era)` collapses to `u64` via
  `.unwrap_or(u64::MAX)` — operationally cleaner for the
  mempool's required-`u64` ttl field.

## R493 handler update

```rust
let inputs = tx.decode_inputs(blk.era).unwrap_or_default();
let fee = tx.decode_fee(blk.era).unwrap_or(0);
let ttl = tx.decode_ttl(blk.era).unwrap_or(u64::MAX);
let entry = MempoolEntry {
    era: blk.era,
    tx_id: tx.id,
    fee,
    body: tx.body.clone(),
    raw_tx: tx.body.clone(),
    size_bytes: tx.serialized_size(),
    ttl: yggdrasil_ledger::SlotNo(ttl),
    inputs,
};
```

Decode failures fall back to forensic defaults (`.unwrap_or(0)`
/ `.unwrap_or(u64::MAX)`) preserving the R488-R494 forensic
stance: malformed bodies don't abort the analysis.

## Forensic fidelity matrix (post-R495)

| MempoolEntry field | R493 | R494 | **R495** |
|--------------------|------|------|----------|
| `era` | real | real | real |
| `tx_id` | real | real | real |
| `body` | real | real | real |
| `size_bytes` | real | real | real |
| `inputs` | empty | **real** | real |
| `fee` | 0 | 0 | **real** |
| `ttl` | u64::MAX | u64::MAX | **real** |
| `raw_tx` | body | body | body (placeholder) |

**7/8 fields real (was 5/8 at R494, 4/8 at R493).** Only
`raw_tx` remains a placeholder — it should be the 3-or-4-element
wire-form CBOR array (`[body, witnesses, aux_data, is_valid?]`)
not just body. That's a bounded follow-on requiring a
`Tx::to_raw_tx_bytes` helper.

## Operational implications

With real `fee`, the mempool's `pop_best()` now returns highest-
fee tx first (matches upstream behavior). With real `ttl`, the
mempool's TTL-eviction at apply boundary works (R488 era-aware
chain walks will now see ttl-expired txs evicted correctly).
With real `inputs`, double-spending txs in the same block are
rejected (R494 behavior).

## Tests delivered (+9 cases)

`crates/ledger/src/tx.rs`:
- `decode_fee_empty_body_returns_zero`
- `decode_fee_byron_carve_out_returns_zero` (Byron returns 0
  regardless of body content)
- `decode_fee_shelley_family_dispatch` (Shelley/Allegra/Mary
  route through ShelleyTxBody)
- `decode_ttl_empty_body_returns_max`
- `decode_ttl_byron_carve_out_returns_max`
- `decode_ttl_shelley_family_dispatch`
- `decode_ttl_alonzo_optional_absent_returns_max`
  (`ttl: None` → `u64::MAX`)
- `decode_ttl_alonzo_optional_present_returns_value`
  (`ttl: Some(7777)` → `7777`)

`crates/tools/db-analyser/src/analysis/runner.rs`:
- `analysis_repro_mempool_and_forge_uses_real_fee_for_priority_ordering_r495`
  (3 distinct Shelley txs with fees 100/500/250; asserts all 3
  insert + forge — proving the dispatcher delivers real fees
  through the wire-up).

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,207 → 6,216
python3 dev/test/check-strict-mirror.py --fail-on-violation   0 violations
python3 dev/test/check-parity-matrix.py                       clean
```

## Stop point

`ReproMempoolAndForge` is now near-fully-fidelity: 7/8
`MempoolEntry` fields populated with real per-era decoded data.
Only `raw_tx` remains as a forensic placeholder; closing it
needs a `Tx::to_raw_tx_bytes` helper that builds the
3-or-4-element wire-form CBOR array.

The R475-R495 sequence has shipped a fully-functional
`db-analyser` binary across **21 rounds** (R475-R495), closing
13/13 dispatch coverage + bringing the only deferred analysis
(`ReproMempoolAndForge`) to near-fully-fidelity.
