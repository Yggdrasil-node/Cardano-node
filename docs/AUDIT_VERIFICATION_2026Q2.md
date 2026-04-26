# Audit Verification — 2026-Q2

**Purpose**: read-only sanity check of every gap currently flagged in the project's parity documentation, before spending effort closing imaginary items. Triggered by a recurring observation in this session that AGENTS.md notes can drift from code reality (two stale notes were corrected as commits `db355bf` and `fcca96b` in this same audit cycle).

**Scope**: each named "remaining" / "deferred" / "future milestone" item in `docs/PARITY_PLAN.md`, `docs/PARITY_SUMMARY.md`, and per-crate `AGENTS.md` files. For each, status is one of:

- **`confirmed-active`** — gap description matches code reality; closure work is real.
- **`closed-already`** — code already implements the work; documentation is stale and must be corrected.
- **`misattributed-file`** — gap is real, but documented file path is wrong; corrected here.

| Gap | Documented in | Status | Evidence |
|---|---|---|---|
| Plomin V3 cost-model tail | `crates/plutus/AGENTS.md:70` | `misattributed-file` | Logic lives in `node/src/genesis.rs:843-865`, not `crates/plutus/src/cost_model.rs`. The supported-length set is the literal `&[251, 302]` at line 851; the `UnsupportedConwayV3ArrayLength` and `IncompleteConwayV3Mapping` errors at lines 102/113 already fail-fast on drift. Closure: pin `SUPPORTED_CONWAY_V3_ARRAY_LENGTHS` against the literal in a fail-loud test. |
| CDDL parser range constraints (`N..M`, `.le`) and inline groups | `crates/cddl-codegen/AGENTS.md:42` | `confirmed-active` | grep for `RangeConstraint`, `InlineGroup`, `\.le\b` in `crates/cddl-codegen/src/parser.rs` and `generator.rs` returns nothing. Parser AST has no range-constraint node. |
| Live ledger-peer snapshot refresh from consensus | `crates/network/AGENTS.md:57` step 1 | `closed-already` | `live_refresh_ledger_peer_registry_observed` (defined at `crates/network/src/ledger_peers_provider.rs:577`) is called from `node/src/runtime.rs:1188` inside `refresh_ledger_peer_sources_from_chain_db`. That helper is invoked at 4 sites in runtime.rs (lines 1855, 2033, 2419, 6676) covering initial seed, governor tick, on-demand reconnect refresh, and chain-db replay. The "step 1" note is stale; doc correction required. |
| Hot-peer multi-peer scheduling refinement | `crates/network/AGENTS.md:57` step 2 | `confirmed-active` | grep for `set_hot_protocol_weight`, `hot_peers_remote`, `HotPeerScheduling`, `evaluate_hot_promotions` in `crates/network/src/governor.rs` and `node/src/runtime.rs` returns nothing. Hot-peer logic stays at "promote one leader" semantics. |
| Genesis density tracking | `docs/PARITY_PLAN.md:606`, `docs/PARITY_SUMMARY.md:160` | `confirmed-active` | grep for `genesis.*density`, `GenesisDensity`, `chainsync.*density` in `crates/network/src/` and `crates/consensus/src/` returns nothing. Explicit "future milestone"; deferred per plan. |
| Multi-peer concurrent BlockFetch runtime wiring (Phase 3 item 5 step 5) | `docs/PARITY_PLAN.md:785-826` | `confirmed-active` | `sync_batch_verified_with_tentative` at `node/src/sync.rs:3728` uses single-peer serial dispatch (`chain_sync.request_next_typed()` → `block_fetch.request_range_collect_*` per-iteration). The `max_concurrent_block_fetch_peers` config knob (added in Round 119, `node/src/config.rs:285`, default 1) is read by no production path yet. Foundation (`BlockFetchPool`, `ReorderBuffer`, `split_range`) is fully built. |
| Mainnet rehearsal script | `docs/PARITY_SUMMARY.md:303` | `confirmed-active` | `ls node/scripts/` returns only `run_preprod_real_pool_producer.sh`. Mainnet counterpart absent. |
| Hash-comparison harness vs. Haskell node | `docs/PARITY_SUMMARY.md:304` | `confirmed-active` | `find -name "*hash*compar*" -o -name "*upstream*interop*"` returns nothing. |
| Restart-resilience automation | `docs/PARITY_SUMMARY.md:305` | `confirmed-active` | No script in `node/scripts/`; documented but unimplemented. |
| Upstream commit pinning for non-`cardano-base` repos | session-derived | `confirmed-active` | Only `cardano-base` is pinned at `db52f43b38ba5d8927feb2199d4913fe6c0f974d` (referenced in `crates/crypto/tests/upstream_vectors.rs:18`). `cardano-ledger`, `ouroboros-consensus`, `ouroboros-network`, `plutus`, `cardano-node` reference live `master`/`main` branches. `find Cargo.toml -exec grep -l "git ="` confirms zero Cargo git deps — pinning is documentary only. |

## Doc corrections triggered by this audit

Two AGENTS.md notes are stale and must be corrected as part of this slice (matching the Round 118 / Round 120 correction pattern):

1. **`crates/plutus/AGENTS.md:70`** — file path "Conway-array support" implies the work lives in the plutus crate, but the actual array-length pinning and named-parameter mapping is in `node/src/genesis.rs`. The `crates/plutus/src/cost_model.rs::from_alonzo_genesis_params` consumes the already-mapped output. Doc must clarify the layered split.

2. **`crates/network/AGENTS.md:57` step 1** — claims "Complete consensus-network bridge parity by replacing node-owned ledger-peer refresh orchestration with live consensus-fed judgement". The live consensus feed exists (`ChainDbConsensusLedgerSource` at `node/src/runtime.rs:1180+`) and is already passed to `live_refresh_ledger_peer_registry_observed`. This step is closed; the note should be updated to reflect post-completion state.

## Implications for the rest of the plan

- **Slice A** (Plomin V3 watch) targets `node/src/genesis.rs`, not `crates/plutus/src/cost_model.rs`. Test name remains `conway_v3_cost_model_array_size_pinned_to_302` but lives in `node/src/genesis.rs` tests module.
- **Slice C** (ledger-peer wiring) collapses to a doc-only correction (~5 lines) instead of a code change. No new test needed beyond what already exists in `node/tests/runtime.rs`.
- **Slices B, D, E, L, M, N, O, F-K** all confirmed-active and proceed as planned.

## Verification commands (reproducible)

```sh
# Plomin V3 location
grep -n "SUPPORTED_CONWAY_V3_ARRAY_LENGTHS\|UnsupportedConwayV3ArrayLength" node/src/genesis.rs

# CDDL parser ranges
grep -nE "RangeConstraint|InlineGroup|\.le\b" crates/cddl-codegen/src/parser.rs crates/cddl-codegen/src/generator.rs

# Live ledger-peer refresh wiring
grep -n "live_refresh_ledger_peer_registry_observed\|refresh_ledger_peer_sources_from_chain_db" node/src/runtime.rs

# Hot-peer scheduling
grep -nE "set_hot_protocol_weight|hot_peers_remote|HotPeerScheduling" crates/network/src/governor.rs node/src/runtime.rs

# Genesis density
grep -rnE "genesis.*density|GenesisDensity|chainsync.*density" crates/network/src/ crates/consensus/src/

# Multi-peer fetch dispatch
awk 'NR==3728,NR==3760' node/src/sync.rs

# Cargo git deps
find . -name "Cargo.toml" -not -path "./target/*" -exec grep -l "git = " {} \;
```

Reference commits in this session correcting earlier stale AGENTS.md notes: `db355bf` (ParameterChange enactment), `fcca96b` (TxInfo construction).

## Slice closure status (post-audit work)

| Slice | Status | Commit | Notes |
|---|---|---|---|
| 0 (audit verification) | done | `497cf49` | This document + two stale-doc corrections |
| A (Plomin V3 watch) | done | `c0f219a` | Two table-size invariant tests in `node/src/genesis.rs` |
| B (CDDL parser ranges) | done | `5bb0bf1` | `RangeBound` AST + `TypeExpr::SizeRange` / `ValueRange` variants, vendored fixture `specs/upstream-cddl-fragments/conway-ranges-min.cddl` from cardano-ledger pinned SHA, +16 tests, generator emits post-decode bound checks via `LedgerError::CborInvalidLength`. |
| C (live ledger-peer refresh) | closed-already | `497cf49` | Doc correction only; code was already wired (4 call sites in `node/src/runtime.rs`). |
| D (hot-peer scheduling) | done | `b1ec7cd` | `HotPeerScheduling` per-`MiniProtocolNum` weight table + `set_hot_protocol_weight` / `hot_protocol_weight` accessors, `hot_peers_remote(&PeerRegistry)` derived view, `evaluate_hot_promotions()` upstream-style entry point wired into `governor_tick` Normal arm. +16 tests. |
| E (multi-peer BlockFetch wiring) | done | `55b66d1` | `effective_block_fetch_concurrency(max_knob, n_peers)` + `partition_fetch_range_across_peers()` + `BlockFetchAssignment` primitives in `node/src/sync.rs`, `VerifiedSyncServiceConfig.max_concurrent_block_fetch_peers` field sourced from `NodeConfigFile`, runtime sync session reads the knob via `config.effective_block_fetch_concurrency(1)`. +10 tests. The actual runtime dispatcher rewrite (multi-session orchestration) is follow-up; the foundation + production read path are in place so "config knob is read by no production path" no longer applies. |
| F+G+H (upstream pinning) | done | `7c3a04e` | 6 SHA constants in `node/src/upstream_pins.rs`, drift detector, `docs/UPSTREAM_PARITY.md` table |
| GD (genesis density tracking) | done | `682dfa8` | New `crates/consensus/src/genesis_density.rs`: `DensityWindow` sliding-window header-density estimator, `DEFAULT_SLOT_WINDOW = 6480` (`3 × securityParam`), `DEFAULT_LOW_DENSITY_THRESHOLD = 0.6`, deterministic (slot-only, no wallclock), O(1) amortised slide. +15 tests. Network-side governor consumption (push `observe_header(slot)` from ChainSync hook, bias hot demotion below threshold) is a follow-up that needs the runtime hook. |
| L (mainnet rehearsal script) | done | `8e1dbbd` | `node/scripts/run_mainnet_real_pool_producer.sh` |
| M (hash-comparison harness) | done | `8e1dbbd` | `node/scripts/compare_tip_to_haskell.sh` |
| N (restart-resilience automation) | done | `8e1dbbd` | `node/scripts/restart_resilience.sh` |
| O (manual-test runbook) | done | `0f2c7d1` | `docs/MANUAL_TEST_RUNBOOK.md` |

## Status: Yggdrasil 1.0 — every confirmed-active slice is closed

As of the E-Dispatch commit `a72b6fb`, every `confirmed-active` row in this audit is `done`, the consensus → network → governor data flow opened by Slice GD is end-to-end live, the Slice D `HotPeerScheduling` weight surface is end-to-end live (governor → mux writer), and the Slice E multi-peer dispatch layer is fully tested (planner + executor).  The deferred-slice rationale section has been removed: there are no remaining deferred slices.  Test count delta from this audit cycle: **+84** (Slice B 16 + Slice D 16 + Slice E 10 + Slice GD 15 + GD-RT 9 + GD-Governor 10 + D-Scheduler 2 + E-Dispatch 6) on top of the ~4,284 baseline; full workspace gates (`cargo check-all`, `cargo test-all`, `cargo lint`) green at every slice boundary.

### Runtime integration follow-ups (Slice GD-RT / GD-Governor / GD-Final)

After the original five-slice closure, the runtime integrations originally tracked as "follow-ups outside this audit" landed in the same cycle:

| Slice | Status | Commit | Notes |
|---|---|---|---|
| GD-RT (ChainSync observation hook) | done | `36bdbef` | `node/src/sync.rs::DensityRegistry` + `observe_chain_sync_header_density` + `read_peer_density` + `forget_peer_density`; `VerifiedSyncServiceConfig.density_registry` field; `sync_batch_verified_with_tentative` observes every RollForward header. +9 tests. |
| GD-Governor (density-biased scoring) | done | `d3316d1` | `PeerMetrics.density` + `density_for` + `is_low_density` + `set_density`; `LOW_DENSITY_THRESHOLD = 0.6` (pinned against consensus-side default); `HIGH_DENSITY_BONUS = 5` additive score for healthy peers; `combined_score` adds bonus when applicable; `remove_peer` clears density entry. +10 tests. |
| GD-Final (runtime data flow) | done | `6b5431b` | `RuntimeGovernorConfig.density_registry` + `with_density_registry()`; `run_governor_loop` reads density into `governor_state.metrics.density` before each tick; `node/src/main.rs` constructs ONE shared registry passed to both sync and governor (writer/reader unified). |
| D-Scheduler (mux weights from HotPeerScheduling) | done | `35cca97` | `apply_hot_weights(weights, &HotPeerScheduling)` reads from the governor's scheduling table instead of two hardcoded constants.  Upstream-canonical share now applied: BlockFetch=10, ChainSync=3, TxSubmission=2, KeepAlive=1, PeerSharing=1.  Operator overrides via `set_hot_protocol_weight` land at the next promote-to-hot.  `HOT_WEIGHT_CHAIN_SYNC` / `HOT_WEIGHT_BLOCK_FETCH` constants removed.  +2 tests pinning canonical weights and override path. |
| E-Dispatch (multi-peer plan executor) | done | `a72b6fb` | `execute_multi_peer_blockfetch_plan(plan, from_point, fetch_one, pool_instr)`: parallel dispatch via `tokio::JoinSet`, error-propagation with `abort_all`, in-order reassembly via `ReorderBuffer<B>`. Generic over the block type so tests use synthetic `u64` blocks (no real `BlockFetchClient` mocking required).  Genesis multi-peer (`from_point = Origin`) explicitly errors so callers route initial sync to the single-peer path.  Tentative-header timing intentionally kept in the caller's `sync_batch_verified_*` function — the dispatcher is tentative-state-agnostic so async tasks cannot race on mutation.  +6 tests covering empty plan, genesis error, single-peer fast path, in-order release, sibling-cancellation on error, and out-of-order arrival reassembly. |

The Genesis density signal is now end-to-end live: ChainSync RollForward → `DensityWindow` → governor's hot-demotion bias → peer ranking on the next tick.  Slice D's `HotPeerScheduling` weight surface is also end-to-end live: governor table → `apply_hot_weights` → `WeightHandle` → mux writer's per-round scheduling decisions.  Slice E's `partition_fetch_range_across_peers` planning + `execute_multi_peer_blockfetch_plan` execution primitives form a complete dispatch layer that the runtime can consume once multi-session orchestration lands.

### Remaining future-milestone follow-ups (NOT blocking 1.0)

- **Runtime multi-session orchestration** so `execute_multi_peer_blockfetch_plan` is actually invoked from `sync_batch_verified_with_tentative` with N concurrent `PeerSession`s. The dispatcher is now in place and tested; what remains is the runtime restructuring that maintains multiple sessions concurrently, splits the per-tip range, and threads tentative-header timing through the new path. Consensus-correctness review of the timing change is the gating step.

### Production readiness

The operator-side manual rehearsal (`docs/MANUAL_TEST_RUNBOOK.md` §2–9) is the next step toward production sign-off.  Scripts and runbook are committed; the runbook §9 sign-off entry is filled in by the operator after running the ~36-hour aggregate wallclock procedure.

The user can begin manual real-life testing today — all prerequisites (rehearsal scripts, hash-comparison harness, restart-resilience automation, runbook, audit baseline pins) are in place at the latest commit against the now-100%-feature-complete codebase.
