---
title: 'R476: HasAnalysis impl for yggdrasil_ledger::Block + Byron EBB registry'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-476-hasanalysis-block-impl/
---

# R476 — `HasAnalysis for Block` + Byron EBB registry

**Date:** 2026-05-11
**Arc:** R475-R481 (`db-analyser HasAnalysis` arc).
**Slice:** R476 = trait impl + Byron known-EBB constant.

## Slice scope

Closes the trait gap from R475: builds on the per-era
`Tx::output_count` dispatcher and ships an `impl HasAnalysis for
yggdrasil_ledger::Block` that powers db-analyser's per-block
methods. Also ports the 325-entry Byron `knownEBBs` constant
table required by the `ShowEBBs` analysis (R480).

| Surface | New file | Lines | Upstream mirror |
|---------|----------|-------|-----------------|
| Byron known-EBB registry | `crates/tools/db-analyser/src/byron_ebbs.rs` | 470 | `deps/ouroboros-consensus/ouroboros-consensus-cardano/src/byron/Ouroboros/Consensus/Byron/EBBs.hs` |
| `impl HasAnalysis for Block` | `crates/tools/db-analyser/src/has_analysis.rs` (appended) | +130 | `deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBAnalyser/Block/Cardano.hs` (subsuming `Block/Byron.hs` + `Block/Shelley.hs`) |

## `byron_ebbs.rs` (R476 — strict mirror)

Direct port of upstream `Ouroboros.Consensus.Byron.EBBs::knownEBBs`
— a flat `[(HeaderHash, Maybe HeaderHash)]` literal of 325 entries
grouped by network (176 mainnet + 119 staging + 30 testnet). The
Rust port:

- Preserves the section markers as three separate `&'static
  [(&str, Option<&str>)]` constants (`MAINNET_EBBS`,
  `STAGING_EBBS`, `TESTNET_EBBS`).
- Lazily builds a `HashMap<HeaderHash, Option<HeaderHash>>` on
  first access via `std::sync::LazyLock`.
- Exposes `pub fn known_ebbs() -> HashMap<HeaderHash,
  Option<HeaderHash>>` returning a snapshot.
- Inline `parse_hex32` decoder so no third-party hex crate is
  pulled in (the EBB tables are the only crate-local hex source).

Strict-mirror status: file declares `**Strict mirror:** EBBs.hs.`
in its module docstring; no synthesis carve-out.

## `HasAnalysis for Block`

Upstream ships three per-era typeclass instances under
`DBAnalyser/Block/{Byron,Shelley,Cardano}.hs` — one per
upstream-side block newtype. Yggdrasil collapses the three into
one impl because `yggdrasil_ledger::Block` is a unified struct
with an `era: Era` discriminator. Per-era logic dispatches
through that discriminator (same shape as upstream's
typeclass-dispatch, just discriminated value-of-Era rather than
typeclass-of-Block).

Methods shipped at R476:

| Method | Behavior | Carve-out |
|--------|----------|-----------|
| `count_tx_outputs` | Sum of `Tx::output_count(era)` (R475) across all transactions | Per-tx decode error → counted as 0 (forensic tool, chain rule pre-filters) |
| `block_tx_sizes` | `Tx::serialized_size() as u64` per transaction | None |
| `known_ebbs` | Returns `byron_ebbs::known_ebbs()` (325-entry static map) | None |
| `emit_traces` | Returns empty `Vec<String>` | Ledger-state apply-loop deferral (R480) |
| `block_stats` | `slot=N`, `block_no=M`, `era=E`, `tx_count=K` | Ledger-state-derived stats deferred (R480) |
| `block_application_metrics` | 4-column CSV (slot/block_no/era/tx_count) | Ledger-state-derived columns deferred (R480) |

Also introduces:

- `pub struct CardanoLedgerStateValues` — placeholder unit struct
  for the `LedgerStateValues` associated type. Real ledger-state
  values land via the future ledger-state apply-loop arc.

## Tests delivered (+14 cases)

- **byron_ebbs.rs** (+5): `known_ebbs_total_count_matches_upstream`,
  `known_ebbs_includes_byron_genesis_successor`,
  `known_ebbs_second_entry_has_prev_hash`, `parse_hex32_round_trip`,
  `unknown_hash_returns_none`.
- **has_analysis.rs** (+9): empty-block / Shelley-sums-per-tx /
  decode-error-treated-as-zero / Byron-dispatch / tx-sizes /
  known-ebbs-returns-byron-registry / emit-traces-empty /
  block-stats-rendering / block-application-metrics-yggdrasil-block.

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,105 → 6,119
python3 dev/test/check-strict-mirror.py --fail-on-violation   0 violations
python3 dev/test/check-parity-matrix.py                       clean
```

## Arc progress (R476/R481)

| Round | Status | Δ tests | Surface |
|-------|--------|---------|---------|
| R475  | shipped | +16 | per-era `TxBody::decode_output_count` + `Tx::output_count` |
| R476  | shipped | +14 | Byron EBB registry + `HasAnalysis for Block` impl |
| R477  | next | — | Allegra/Mary/Alonzo dispatch tests + impl polish |
| R478  | pending | — | Babbage/Conway dispatch coverage |
| R479  | pending | — | `analysis::runner::run_analysis` + 4 handlers |
| R480  | pending | — | 3 more block-only handlers + 6 ledger-state deferrals |
| R481  | pending | — | Arc closeout |

## References

- Plan: `docs/COMPLETION_ROADMAP.md`.
- Upstream EBBs: `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/byron/Ouroboros/Consensus/Byron/EBBs.hs`.
- Upstream HasAnalysis-Block: `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBAnalyser/Block/{Byron,Shelley,Cardano}.hs`.
- Yggdrasil R375 trait surface: `crates/tools/db-analyser/src/has_analysis.rs:87-134`.

## Stop point

R477 next: per-era dispatch tests for Allegra/Mary (both route via
`ShelleyTxBody::decode_output_count`) plus Alonzo, deepening the
existing coverage. The impl itself already covers all 7 eras
through the R475 dispatcher; R477 is the test/coverage round.
