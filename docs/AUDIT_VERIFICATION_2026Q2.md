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
| B (CDDL parser ranges) | **deferred** | — | No current CDDL consumer in the workspace uses `N..M` / `.le N` constraints (only `specs/mini-ledger.cddl` exists and uses `.size`). Implementing speculative parser support without a fixture means the feature can't be validated end-to-end. Re-evaluate when upstream `cardano-ledger` ships a CDDL file that requires it. |
| C (live ledger-peer refresh) | closed-already | `497cf49` | Doc correction only; code was already wired (4 call sites in `node/src/runtime.rs`). |
| D (hot-peer scheduling) | **deferred** | — | Multi-slice work that does not block manual mainnet testing on the proven single-peer-leader pipeline. Track as follow-up. |
| E (multi-peer BlockFetch wiring) | **deferred** | — | Multi-day item explicitly noted in the plan. The Round 119 config knob (`max_concurrent_block_fetch_peers`, default 1) is the entry point; the runtime dispatcher in `node/src/sync.rs::sync_batch_verified_with_tentative` is the next step. Track as follow-up. |
| F+G+H (upstream pinning) | done | `7c3a04e` | 6 SHA constants in `node/src/upstream_pins.rs`, drift detector, `docs/UPSTREAM_PARITY.md` table |
| L (mainnet rehearsal script) | done | `8e1dbbd` | `node/scripts/run_mainnet_real_pool_producer.sh` |
| M (hash-comparison harness) | done | `8e1dbbd` | `node/scripts/compare_tip_to_haskell.sh` |
| N (restart-resilience automation) | done | `8e1dbbd` | `node/scripts/restart_resilience.sh` |
| O (manual-test runbook) | done | `0f2c7d1` | `docs/MANUAL_TEST_RUNBOOK.md` |

## Deferred-slice rationale

- **Slice B (CDDL ranges)**: speculative without a real fixture. The cost of implementing without a consumer is unvalidated parser code that may need to be rewritten when an actual upstream CDDL file lands. Better to wait for a concrete need.
- **Slices D + E (multi-peer fetch)**: multi-day scope explicitly noted at plan time. The proven single-peer pipeline meets manual-testing needs. The infrastructure (`BlockFetchPool`, `ReorderBuffer`, `split_range`, `max_concurrent_block_fetch_peers` config knob) is in place; what's deferred is the runtime dispatcher that actually uses N peers.

The user can begin manual real-life testing today — all prerequisites (rehearsal scripts, hash-comparison harness, restart-resilience automation, runbook, audit baseline pins) are in place at commit `7c3a04e` against the as-is 99% codebase.
