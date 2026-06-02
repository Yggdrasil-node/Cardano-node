---
title: 'R498-R510: node/src/sync.rs filename-mirror split arc plan'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-12-round-498-r510-sync-rs-split-plan/
---

# R498–R510 — `node/src/sync.rs` split arc plan

**Date:** 2026-05-12
**Predecessor:** [`R497`](2026-05-11-round-497-to-raw-tx-bytes-fidelity.md) closed (db-analyser HasAnalysis arc complete, 13/13 dispatch coverage, 8/8 MempoolEntry fields real).
**Decision document:** this file. Awaiting per-round `proceed` authorization.

## Summary

`node/src/sync.rs` is the largest single file in the workspace at
**9,579 LOC** (production; tests at the bottom). The file's own
docstring already commits to a split:

> Yggdrasil collapses the runtime service into one large module that
> R271-arc rounds split incrementally into `runtime/*.rs` sub-files.

`runtime.rs` already shipped that split (R271 arc); `sync.rs` has
not. Every other oversized file in `node/src/` (`runtime.rs`,
`config.rs`, `genesis.rs`, `commands.rs`, `local_server.rs`,
`server.rs`, `handlers.rs`, `plutus_eval.rs`) already lives next to a
same-named sub-directory. `sync.rs` should follow the same convention
into `node/src/sync/`.

This is a **filename-mirror extraction** per
`docs/AGENTS.md`:
behavior-preserving carve-up into upstream-aligned sub-modules. The
file is runtime glue (`**Strict mirror:** none.`) — most sub-files
will keep the synthesis form but should name the upstream
mini-protocol / consensus module they sequence so future readers can
locate the reference Haskell.

**13 rounds proposed.** Each round = one bounded slice, four cargo
gates green, one operational-runs doc, one commit. No behavior
changes — visibility moves from `pub` / `pub(crate)` only as required
to keep external callers compiling.

## Current section inventory

Line ranges in HEAD `node/src/sync.rs`:

| Range | Phase header | Surface | LOC |
|---|---|---|---|
| 1–205     | (preamble) | `SyncError`, imports, `DIJKSTRA_MAJOR_PROTOCOL_VERSION` | 205 |
| 206–445   | (decoders) | `compute_tx_id`, `shelley_block_to_block(+_with_spans)`, `decode_shelley_blocks`, `decode_shelley_header`, `decode_point` | 240 |
| 446–845   | (fetch primitives) | `map_blockfetch_error`, `fetch_range_blocks*` (raw / typed / multi-era / decoded), `normalize_blockfetch_range_*`, `point_from_raw_header`, `point_bytes_from_raw_header_or_tip` | 400 |
| 846–1167  | (typed sync API) | `sync_step*`, `sync_steps*`, `sync_until_typed`, `apply_typed_*_to_volatile`, `typed_find_intersect`, `sync_batch_apply`, `keepalive_heartbeat` | 322 |
| 1168–1261 | **Phase 33** managed sync service | `run_sync_service`, `SyncServiceConfig`, `SyncServiceOutcome` | 94 |
| 1262–3291 | **Phase 37** verified sync service core | `VerifiedSyncServiceConfig`, `LedgerCheckpointPolicy`, chain-dep sidecar (`encode/decode/load_*_sidecar_snapshot`, `chain_dep_context_from_sidecar`), perf/overlay (`apparent_performance_ratio`, `compute_pool_performance`, `compute_epoch_boundary_pool_performance`, `is_tpraos_block`, `overlay_step`, `is_overlay_slot`, `lookup_tpraos_overlay_schedule`), `advance_ledger_with_epoch_boundary`, `apply_nonce_evolution_to_progress`, `update_ledger_checkpoint_after_progress`, `default_checkpoint_tracking`, `recover_ledger_state_chaindb*`, `volatile_replay_blocks_after`, `replay_storage_block_*`, `replay_chain_dep_*`, `restore_chain_dep_*`, `run_verified_sync_service*`, `persist_chain_dep_state_sidecar` | **2,030** |
| 3292–3435 | **Phase 34** consensus header bridge | `shelley_opcert_to_consensus`, `shelley_header_body_to_consensus`, `shelley_header_to_consensus`, `verify_shelley_header`, `praos_header_body_to_consensus`, `praos_header_to_consensus`, `verify_praos_header` | 144 |
| 3436–3867 | **Phase 35** multi-era block decode | `MultiEraBlock`, `decode_multi_era_block(s)`, header-hash helpers, `era_tag` mod | 432 |
| 3868–5225 | **Phase 37b** verified multi-era sync pipeline | `multi_era_block_to_block`, `verify_multi_era_block`, `sync_step_multi_era`, `sync_batch_apply_verified`, `VerificationConfig`, era converters with spans, future-block check, body-hash + max-major-protver + HeaderProtVerTooHigh wiring | 1,358 |
| 5226–6167 | **Phase 42** ChainState integration | `multi_era_block_to_chain_entry`, `track_chain_state`, `promote_stable_blocks`, sidecar-aware rollback + verified ChainDb runner (`run_verified_sync_service_chaindb` siblings), `dispatch_range_with_tentative`, tentative-state plumbing | **942** |
| 6168–6341 | **Phase 40** mempool sync eviction | `extract_tx_ids`, `evict_confirmed_from_mempool`, `evict_mempool_after_roll_forward`, `collect_rolled_back_tx_ids` | 174 |
| 6342–6416 | **Slice GD-RT** density observation | `DensityRegistry`, `DensityWindow`, `observe_chain_sync_header_density`, `read_peer_density`, `forget_peer_density` | 75 |
| 6417–7057 | **Slice E** multi-peer BlockFetch dispatch | `effective_block_fetch_concurrency`, `BlockFetchAssignment`, `partition_fetch_range_across_peers`, `execute_multi_peer_blockfetch_plan(_inline)`, `ReorderBuffer`, `dispatch_range_with_tentative`, `MultiPeerDispatchContext` | 641 |
| 7058–9579 | `mod tests` | (in-place; not split) | 2,522 |

**Production total:** ~7,057 LOC. **Tests:** ~2,522 LOC. Tests stay
in `sync.rs` until the production split lands, then move into
per-sub-module test files in a follow-up round per the
round-extraction recipe.

## Proposed round breakdown

13 rounds, each one bounded slice, behavior-preserving, four cargo
gates green.

| Round | Slice | New file(s) under `node/src/sync/` | Upstream affinity | LOC |
|---|---|---|---|---|
| **R498** | Create `node/src/sync/` directory; move preamble (imports + `SyncError` + `DIJKSTRA_MAJOR_PROTOCOL_VERSION`) into `sync/error.rs`. `sync.rs` becomes module-rooting `mod error; pub use error::*;`. No public-API rename. | `sync/error.rs` | (synthesis — error glue) | 205 |
| **R499** | Extract decoders: `compute_tx_id` + `shelley_block_to_block(+_with_spans)` + `decode_shelley_*` + `decode_point` into `sync/shelley_decoders.rs`. | `sync/shelley_decoders.rs` | `Ouroboros.Consensus.Shelley.Ledger.Block` (`decodeShelleyBlock`) | 240 |
| **R500** | Extract BlockFetch fetch primitives (`fetch_range_blocks*` family, `map_blockfetch_error`, `normalize_blockfetch_range_*`, `point_from_raw_header`, `point_bytes_from_raw_header_or_tip`) into `sync/block_fetch.rs`. | `sync/block_fetch.rs` | `Ouroboros.Network.BlockFetch.Client` + `Ouroboros.Consensus.MiniProtocol.BlockFetch.Client` | 400 |
| **R501** | Extract typed ChainSync sync API (`sync_step*`, `sync_steps*`, `sync_until_typed`, `apply_typed_*_to_volatile`, `typed_find_intersect`, `sync_batch_apply`) into `sync/chain_sync.rs`. | `sync/chain_sync.rs` | `Ouroboros.Consensus.MiniProtocol.ChainSync.Client` | 322 |
| **R502** | Extract `keepalive_heartbeat` + Phase 33 managed sync service (`run_sync_service`, `SyncServiceConfig`, `SyncServiceOutcome`) into `sync/service.rs`. | `sync/service.rs` | `Ouroboros.Consensus.Node.Run` (sync side) | 188 |
| **R503** | Extract Phase 34 consensus header bridge (`shelley_/praos_*_to_consensus`, `verify_shelley_header`, `verify_praos_header`) into `sync/header_bridge.rs`. | `sync/header_bridge.rs` | `Ouroboros.Consensus.Protocol.{TPraos,Praos}.Translate` | 144 |
| **R504** | Extract Phase 35 multi-era decode (`MultiEraBlock`, `decode_multi_era_block(s)`, header-hash helpers, `era_tag` mod) into `sync/multi_era_decode.rs`. | `sync/multi_era_decode.rs` | `Ouroboros.Consensus.Cardano.Block` (`CardanoBlock`) | 432 |
| **R505** | Extract Phase 37b verified multi-era sync pipeline (`multi_era_block_to_block`, `verify_multi_era_block`, `sync_step_multi_era`, `sync_batch_apply_verified`, `VerificationConfig`, era converters with spans, future-block check, body-hash + max-major-protver + HeaderProtVerTooHigh wiring) into `sync/verified.rs`. **Largest single round** — likely warrants R505a / R505b split if review surfaces a clean seam. | `sync/verified.rs` | `Ouroboros.Consensus.Cardano.Node` (verified sync) | 1,358 |
| **R506** | Extract chain-dep sidecar helpers from Phase 37 (`encode/decode/load_*_sidecar_snapshot`, `chain_dep_context_from_sidecar`, `load_stake_snapshots_sidecar`, `persist_chain_dep_state_sidecar`, `restore_chain_dep_*`, `replay_chain_dep_*`) into `sync/chain_dep_sidecar.rs`. | `sync/chain_dep_sidecar.rs` | `Ouroboros.Consensus.Protocol.Praos.{ChainDepState,Translate}` (synthesis — sidecar codec is Yggdrasil-side) | ~620 |
| **R507** | Extract perf/overlay helpers from Phase 37 (`gcd_*`, `ceil_div_u128`, `apparent_performance_ratio`, `compute_pool_performance`, `compute_epoch_boundary_pool_performance`, `is_tpraos_block`, `unit_interval_is_zero`, `ceil_ratio_u128`, `overlay_step`, `is_overlay_slot`, `lookup_tpraos_overlay_schedule`) into `sync/praos_overlay.rs`. | `sync/praos_overlay.rs` | `Ouroboros.Consensus.Protocol.TPraos` (`pbftVrfChecks`, `overlaySchedule`) | ~250 |
| **R508** | Extract remaining Phase 37 core (ledger checkpoint + epoch-boundary advance + recovery: `LedgerCheckpointPolicy`, `phase2_evaluator_or_trust_block`, `warn_phase2_skip_once`, `for_each_roll_forward_block`, `advance_ledger_state_with_progress`, `advance_ledger_with_epoch_boundary`, `apply_nonce_evolution_to_progress`, `update_ledger_checkpoint_after_progress`, `default_checkpoint_tracking`, `recover_ledger_state_chaindb*`, `storage_point_for_block`, `volatile_replay_blocks_after`, `stake_snapshots_from_ledger_state`, `replay_storage_block_*`, `checkpoint_missing_genesis_cost_models`, `storage_chain_contains_point`, `run_verified_sync_service`, `run_verified_sync_service_chaindb`) into `sync/verified_service.rs`. **Second-largest round** — may also warrant a/b split. | `sync/verified_service.rs` | `Ouroboros.Consensus.Storage.ChainDB.Impl` + `Ouroboros.Consensus.Node.Run` (verified sync runtime) | ~1,160 |
| **R509** | Extract Phase 42 ChainState integration (`multi_era_block_to_chain_entry`, `track_chain_state`, `promote_stable_blocks`, sidecar-aware rollback helpers, `dispatch_range_with_tentative`, tentative-state plumbing) into `sync/chain_state.rs`. | `sync/chain_state.rs` | `Ouroboros.Consensus.Storage.ChainDB.Impl.ChainSel` + `Ouroboros.Network.BlockFetch.Decision.Trace` (tentative-state) | 942 |
| **R510** | Extract Phase 40 mempool eviction + Slice GD-RT density + Slice E multi-peer BlockFetch into 3 leaves: `sync/mempool_eviction.rs`, `sync/density.rs`, `sync/multi_peer_fetch.rs`. Then close-out: shrink `sync.rs` to a pure rerooting module with `pub use` re-exports; relocate `mod tests` blocks into per-leaf test files where mechanical, leave shared integration test in `node/tests/`. | `sync/mempool_eviction.rs`, `sync/density.rs`, `sync/multi_peer_fetch.rs` + closeout | `Ouroboros.Consensus.Mempool.Impl.Update` (`syncWithLedger`); `Ouroboros.Network.BlockFetch.ClientRegistry`; (density: synthesis) | 174 + 75 + 641 |

**Total carved:** ~7,000 LOC into 13 sub-modules. **`sync.rs` final
size:** ~50 LOC (mod declarations + `pub use` re-exports + the
existing top-of-file `## Naming parity` docstring).

## Strict-mirror discipline

Per AGENTS.md and `docs/AGENTS.md`, every
new `.rs` file must carry either:

1. A real upstream `.hs` mirror (snake_case basename match), OR
2. A `## Naming parity` docstring stanza ending in `**Strict mirror:**
   none.` plus the upstream symbol(s)/file(s) the helper surfaces.

Most leaves above are runtime glue; their docstrings will declare
`**Strict mirror:** none.` and cite the upstream mini-protocol /
consensus module they sequence. Two candidates may qualify as direct
mirrors after R504/R503 land — `multi_era_decode.rs` ↔
`Ouroboros.Consensus.Cardano.Block` and `header_bridge.rs` ↔
`Ouroboros.Consensus.Protocol.{TPraos,Praos}.Translate` — but those
upstream files are larger than our extracts, so synthesis-form is the
honest classification. The strict-mirror drift-guard
(`dev/test/check-strict-mirror.py --fail-on-violation`) must stay green
on every round commit.

## Risk register

| Risk | Mitigation |
|---|---|
| Hidden mutual recursion between Phase 37 sidecar code and Phase 37b verified pipeline (R505 ↔ R506) | Sequence R506 before R505. Build a callgraph at R506 entry; if cycles surface, fold the cyclic helper into the larger leaf. |
| `pub(crate)` ↔ `pub` visibility flux when functions cross module boundaries | Adopt the R273 convention: keep `pub(crate)` for everything that doesn't need `pub`; let the compiler tell us which symbols need crate-wide re-export via `sync.rs` `pub use`. |
| `mod tests` tied to internals via `#[cfg(test)]` `pub(crate)` reach-in | R510 closeout moves tests to per-leaf files; if any test exercises a private helper, expose via `#[cfg(test)] pub(crate)` (already standard) rather than restructure the helper. |
| R505 / R508 too large at ~1.2 kLOC each — single-round commits may exceed reviewer attention budget | Reserve R505a/R505b and R508a/R508b as soft-allocated slots; split if the natural seam appears during extraction. |
| Operational-runs documentation overhead (13 docs minimum) | Skill recipe is already encoded; each round-doc uses the template from `docs/AGENTS.md`. |

## Authorization rhythm

Per `tasks/todo.md`, each round
proceeds only on explicit operator `proceed` after the prior round's
four gates pass and operational-runs doc lands. No round runs
preemptively. End-of-arc is R510; expected duration is one
short-session per round given the file's clean phase-banner structure.

## Acceptance criteria for arc closure (R510 closeout)

- `node/src/sync.rs` is ≤ 100 LOC (mod declarations + `pub use` +
  top-of-file docstring).
- 13 leaves under `node/src/sync/` each carry a `## Naming parity`
  stanza.
- `dev/test/check-strict-mirror.py --fail-on-violation` reports 0
  violations.
- All four cargo gates green: `cargo fmt --all -- --check`, `cargo
  check-all`, `cargo lint`, `cargo test-all`.
- Workspace test count unchanged or higher (no test loss during the
  move).
- `docs/strict-mirror-audit.tsv` updated with 13 new `(c) strict-none`
  rows; running tally documented in this file's R510 closeout entry.
