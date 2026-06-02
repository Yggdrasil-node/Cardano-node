---
title: 'R475: per-era TxBody output-count helpers'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-475-tx-output-count-helpers/
---

# R475 — per-era TxBody output-count helpers

**Date:** 2026-05-11
**Arc:** R475-R481 (`db-analyser HasAnalysis` arc, 7 rounds).
**Slice:** R475 = ledger-side preconditions for the
`CountTxOutputs` analysis — a 5-era output-count decoder surface
plus the `Tx::output_count` dispatch helper.

## Slice scope

R475 lands the per-era TxBody output-count decoders that
db-analyser's `CountTxOutputs` analysis (R479) will consume.
The shape mirrors upstream's per-era `HasAnalysis` typeclass
instances under `Cardano.Tools.DBAnalyser.HasAnalysis.{Byron,Shelley,
Allegra,Mary,Alonzo,Babbage,Conway}` — each instance's
`countTxOutputs` walks the era's `TxBody` and sums output-count
fields. Yggdrasil's `Block` is a unified struct with an `era: Era`
discriminator, so the dispatch lives in `Tx::output_count` as a
single `match` arm rather than seven typeclass instances.

| Era family | New helper | New file location |
|------------|-----------|--------------------|
| Byron | `ByronTx::decode_output_count(&[u8]) -> Result<usize, _>` | `crates/ledger/src/eras/byron.rs` (impl block at line 252) |
| Shelley + Allegra + Mary | `ShelleyTxBody::decode_output_count(&[u8]) -> Result<usize, _>` | `crates/ledger/src/eras/shelley.rs` (impl block after `CborDecode`) |
| Alonzo | `AlonzoTxBody::decode_output_count(&[u8]) -> Result<usize, _>` | `crates/ledger/src/eras/alonzo.rs` |
| Babbage | `BabbageTxBody::decode_output_count(&[u8]) -> Result<usize, _>` | `crates/ledger/src/eras/babbage.rs` |
| Conway | `ConwayTxBody::decode_output_count(&[u8]) -> Result<usize, _>` | `crates/ledger/src/eras/conway.rs` |

Each helper is a 4-line wrapper: instantiate a `Decoder` over the
body bytes, call the existing `CborDecode::decode_cbor` path, and
return `body.outputs.len()`. Errors propagate through `LedgerError`.

## Dispatcher (`Tx::output_count`)

`crates/ledger/src/tx.rs:113` adds a per-`Era` dispatch method:

```rust
pub fn output_count(&self, era: Era) -> Result<usize, LedgerError> {
    if self.body.is_empty() { return Ok(0); }
    match era {
        Era::Byron => crate::eras::byron::ByronTx::decode_output_count(&self.body),
        Era::Shelley | Era::Allegra | Era::Mary => {
            crate::eras::shelley::ShelleyTxBody::decode_output_count(&self.body)
        }
        Era::Alonzo => crate::eras::alonzo::AlonzoTxBody::decode_output_count(&self.body),
        Era::Babbage => crate::eras::babbage::BabbageTxBody::decode_output_count(&self.body),
        Era::Conway => crate::eras::conway::ConwayTxBody::decode_output_count(&self.body),
    }
}
```

Empty-body input returns `Ok(0)` (matches upstream's
`countTxOutputs (Block { blkTxs = [] }) = 0` short-circuit);
malformed bodies propagate `LedgerError::CborDecodeError` rather
than panicking or silently returning zero.

## Tests delivered

5 per-era tests + 5 dispatcher tests = 16 new test cases:

| File | New tests | Coverage |
|------|-----------|----------|
| `crates/ledger/src/eras/byron.rs` | 4 (`_zero_outputs`, `_single_output`, `_multiple_outputs`, `_rejects_malformed_body`) | Byron 3-element-array body shape |
| `crates/ledger/src/eras/shelley.rs` | 3 (`_single_output`, `_multiple_outputs`, `_rejects_malformed_body`) | Shelley/Allegra/Mary map-keyed body shape |
| `crates/ledger/src/eras/alonzo.rs` | 3 (same shape) | Alonzo body shape with Value enum |
| `crates/ledger/src/eras/babbage.rs` | 3 (same shape) | Babbage body shape with inline datums + ref-inputs |
| `crates/ledger/src/eras/conway.rs` | 3 (same shape) | Conway body shape with voting/proposal procedures |
| `crates/ledger/src/tx.rs` | 5 (`_empty_body_returns_zero`, `_byron_dispatch`, `_shelley_family_dispatch`, `_alonzo_dispatch`, `_dispatch_propagates_decode_error`) | Per-era dispatch + empty-body short-circuit + error propagation |

## Naming-parity stance

This is **not** a strict 1:1 file-mirror split — R475 adds a
per-era helper surface alongside existing `CborDecode` impls. The
`Tx::output_count` dispatcher's docstring carries the upstream
reference (`Cardano.Tools.DBAnalyser.HasAnalysis::countTxOutputs`
per-era dispatch) and each era's helper docstring cites the
corresponding per-era typeclass instance. No new files; no
strict-mirror manifest entries.

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,084 → 6,105
python3 dev/test/check-strict-mirror.py --fail-on-violation   0 violations
python3 dev/test/check-parity-matrix.py                       clean
```

## Arc progress (R475/R481)

| Round | Status | Δ tests | Surface |
|-------|--------|---------|---------|
| R475  | shipped | +16 | per-era `TxBody::decode_output_count` + `Tx::output_count` |
| R476  | next | — | `impl HasAnalysis for yggdrasil_ledger::Block` (Byron + Shelley dispatch) |
| R477  | pending | — | Allegra/Mary/Alonzo dispatch coverage |
| R478  | pending | — | Babbage/Conway dispatch coverage |
| R479  | pending | — | `analysis::runner::run_analysis` + 4 handlers |
| R480  | pending | — | 3 more block-only handlers + 6 ledger-state deferrals |
| R481  | pending | — | `lib.rs` wire-up + `status::analysis_dispatch_status` closure + arc-closeout doc |

## References

- Plan: `docs/COMPLETION_ROADMAP.md`.
- Upstream: `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBAnalyser/HasAnalysis.hs`
  + per-era `HasAnalysis.{Byron,Shelley,Allegra,Mary,Alonzo,Babbage,Conway}.hs`.
- Yggdrasil trait surface from R373: `crates/tools/db-analyser/src/has_analysis.rs:87-134`.
- `AnalysisName` 13-variant enum from R351: `crates/tools/db-analyser/src/types.rs:117-144`.

## Stop point

R476 next: implement `HasAnalysis for yggdrasil_ledger::Block`
calling into the R475 dispatch helpers, plus the Byron known-EBB
registry (hard-coded from upstream `byronEpochBoundaryBlocks`).
