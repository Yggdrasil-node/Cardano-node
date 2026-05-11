---
title: 'R478: HasAnalysis Babbage / Conway dispatch coverage'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-478-hasanalysis-babbage-conway-coverage/
---

# R478 — HasAnalysis Babbage / Conway dispatch coverage

**Date:** 2026-05-11
**Arc:** R475-R481 (`db-analyser HasAnalysis` arc).
**Slice:** R478 = per-era dispatch coverage for Babbage and Conway.

## Slice scope

Final dispatch-coverage round before R479's analysis runner.
Adds concrete Babbage + Conway dispatch tests using each era's
real TxBody construction, completing the 7-era dispatch coverage
matrix.

## Tests delivered (+6 cases)

| Test name | Coverage |
|-----------|----------|
| `block_count_tx_outputs_babbage_dispatch` | Era::Babbage routes to BabbageTxBody decoder (1-tx × 2-out) |
| `block_count_tx_outputs_babbage_multi_tx` | Per-tx Babbage accumulation (2-tx × 2-out = 4) |
| `block_count_tx_outputs_conway_dispatch` | Era::Conway routes to ConwayTxBody decoder (1-tx × 4-out incl. multi-coin shape) |
| `block_count_tx_outputs_conway_multi_tx` | Per-tx Conway accumulation (3-tx × 4-out = 12) |
| `block_stats_renders_babbage_and_conway` | Era-name rendering for both |
| `block_application_metrics_renders_babbage_and_conway` | All 4 CSV columns render correctly for both |

## Dispatch coverage matrix (post-R478)

| Era | Wire-format TxBody | Test count (R476+R477+R478) |
|-----|---------------------|------------------------------|
| Byron | `ByronTx` | 2 |
| Shelley | `ShelleyTxBody` | 3 |
| Allegra | `ShelleyTxBody` (shared) | 1 |
| Mary | `ShelleyTxBody` (shared) | 1 |
| Alonzo | `AlonzoTxBody` | 3 |
| Babbage | `BabbageTxBody` | 2 |
| Conway | `ConwayTxBody` | 2 |

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,125 → 6,131
python3 scripts/check-strict-mirror.py --fail-on-violation   0 violations
python3 scripts/check-parity-matrix.py                       clean
```

## Arc progress (R478/R481)

| Round | Status | Δ tests | Surface |
|-------|--------|---------|---------|
| R475  | shipped | +16 | per-era `TxBody::decode_output_count` + `Tx::output_count` |
| R476  | shipped | +14 | Byron EBB registry + `HasAnalysis for Block` impl |
| R477  | shipped | +6  | Allegra/Mary/Alonzo dispatch coverage |
| R478  | shipped | +6  | Babbage/Conway dispatch coverage |
| R479  | next | — | `analysis::runner::run_analysis` + 4 handlers |
| R480  | pending | — | 3 more block-only handlers + 6 ledger-state deferrals |
| R481  | pending | — | Arc closeout |

## Stop point

R479 next: build `crates/tools/db-analyser/src/analysis/runner.rs`
with the `run_analysis` dispatch core + 4 block-iteration-only
handlers (ShowSlotBlockNo, CountBlocks, CountTxOutputs,
ShowBlockHeaderSize). Adds the `AnalysisOutcome` /
`AnalysisError` types.
