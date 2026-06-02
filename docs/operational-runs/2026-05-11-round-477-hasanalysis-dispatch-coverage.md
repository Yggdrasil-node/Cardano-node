---
title: 'R477: HasAnalysis Allegra / Mary / Alonzo dispatch coverage'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-477-hasanalysis-dispatch-coverage/
---

# R477 — HasAnalysis Allegra / Mary / Alonzo dispatch coverage

**Date:** 2026-05-11
**Arc:** R475-R481 (`db-analyser HasAnalysis` arc).
**Slice:** R477 = per-era dispatch test coverage for Allegra /
Mary / Alonzo plus a backward-compat observation about Alonzo's
TxBody decoder.

## Slice scope

R476 shipped one `impl HasAnalysis for yggdrasil_ledger::Block`
covering all 7 eras through `Block::era`-discriminated dispatch.
R477 strengthens the test surface by exercising the dispatch on
Allegra/Mary/Alonzo concretely and documenting an empirical
observation about Alonzo's TxBody decoder's backward
compatibility with Shelley's wire format.

| Surface | Tests added |
|---------|-------------|
| Allegra dispatch (ShelleyTxBody-shaped, 2-tx × 2-out) | 1 |
| Mary dispatch (ShelleyTxBody-shaped, 1-tx × 2-out) | 1 |
| Alonzo dispatch (real AlonzoTxBody, 1-tx × 3-out) | 1 |
| Alonzo multi-tx dispatch (3-tx × 3-out) | 1 |
| Alonzo decoder accepts Shelley body (wire-format superset observation) | 1 |
| `block_stats` per-era era-name rendering | 1 |

## Wire-format superset observation

The new test `block_count_tx_outputs_alonzo_decoder_accepts_shelley_body`
documents an empirical property: when a block carries a
`Shelley`-shaped body but is tagged `Era::Alonzo`, the Alonzo
TxBody decoder accepts it and returns the body's output count.
This is *not* a chain-validity claim — real Alonzo blocks always
carry full Alonzo bodies — but it documents that the upstream
Alonzo TxBody CBOR map (keys 0..6 shared with Shelley + optional
keys 7..15 for Alonzo-only fields) is a wire-format superset of
Shelley's, so the decoder's tolerance falls out from the format
spec.

## Tests delivered (+6 cases)

| Test name | Coverage |
|-----------|----------|
| `block_count_tx_outputs_allegra_dispatch` | Era::Allegra routes to ShelleyTxBody decoder |
| `block_count_tx_outputs_mary_dispatch` | Era::Mary routes to ShelleyTxBody decoder |
| `block_count_tx_outputs_alonzo_dispatch` | Era::Alonzo routes to AlonzoTxBody decoder (single-tx with 3 outputs incl. datum-hash) |
| `block_count_tx_outputs_alonzo_multi_tx` | Per-tx accumulation across 3 alonzo txs |
| `block_count_tx_outputs_alonzo_decoder_accepts_shelley_body` | Alonzo decoder accepts Shelley wire format (superset documentation) |
| `block_stats_renders_each_era` | Iterates Allegra/Mary/Alonzo, asserts era-name rendered correctly |

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,119 → 6,125
python3 dev/test/check-strict-mirror.py --fail-on-violation   0 violations
python3 dev/test/check-parity-matrix.py                       clean
```

## Arc progress (R477/R481)

| Round | Status | Δ tests | Surface |
|-------|--------|---------|---------|
| R475  | shipped | +16 | per-era `TxBody::decode_output_count` + `Tx::output_count` |
| R476  | shipped | +14 | Byron EBB registry + `HasAnalysis for Block` impl |
| R477  | shipped | +6  | Allegra/Mary/Alonzo dispatch coverage + wire-format observation |
| R478  | next | — | Babbage/Conway dispatch coverage |
| R479  | pending | — | `analysis::runner::run_analysis` + 4 handlers |
| R480  | pending | — | 3 more block-only handlers + 6 ledger-state deferrals |
| R481  | pending | — | Arc closeout |

## Stop point

R478 next: Babbage/Conway dispatch coverage with real per-era
TxBody construction (including BabbageTxOut inline datums +
ConwayTxBody voting/proposal procedures fields).
