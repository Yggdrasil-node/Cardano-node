## Round 271m — `runtime.rs` per-domain split: thirteenth slice (Reconnecting verified-sync family)

Date: 2026-05-07
Branch: main
Type: Filename-mirror refactor (Phase γ R271 thirteenth slice — twelve-fn cluster extraction)

### Slice scope

Extracted **1,992 source lines** (12 items) from `runtime.rs` into a
new `node/src/runtime/reconnecting_sync.rs` (2,065 lines including
module docstring + imports). Items moved:

- `pub async fn run_reconnecting_verified_sync_service<S, F>(...)`
- `pub async fn run_reconnecting_verified_sync_service_chaindb<I, V, L, F>(...)`
- `pub async fn run_reconnecting_verified_sync_service_with_tracer<S, F>(...)`
- `pub async fn run_reconnecting_verified_sync_service_chaindb_with_tracer<I, V, L, F>(...)`
- `pub async fn resume_reconnecting_verified_sync_service_chaindb<I, V, L, F>(...)`
- `pub async fn resume_reconnecting_verified_sync_service_chaindb_with_tracer<I, V, L, F>(...)`
- `pub async fn resume_reconnecting_verified_sync_service_shared_chaindb<I, V, L, F>(...)`
- `pub async fn resume_reconnecting_verified_sync_service_shared_chaindb_with_tracer<I, V, L, F>(...)`
- `async fn run_reconnecting_verified_sync_service_chaindb_inner<I, V, L, F>(...)`
- `async fn run_reconnecting_verified_sync_service_shared_chaindb_inner<I, V, L, F>(...)`
- `pub(crate) fn stake_snapshots_for_recovered_point(...)`
- `pub(crate) fn recover_ledger_state_for_runtime<I, V, L>(...)`
- `pub(crate) struct RuntimeLedgerRecovery { ... }` (helper struct used by the recovery fn)

`runtime.rs` keeps a `pub mod reconnecting_sync;` declaration and a
`pub use reconnecting_sync::{...}` block that surfaces the eight `run/resume`
entry points so `run_node.rs` and other call sites continue to resolve
`crate::runtime::run_reconnecting_verified_sync_service*` /
`crate::runtime::resume_reconnecting_verified_sync_service*` unchanged. The
two `pub(crate)` helpers + their helper struct are gated behind a
`#[cfg(test)] pub(crate) use reconnecting_sync::{recover_ledger_state_for_runtime, stake_snapshots_for_recovered_point};`
re-export — they are only consumed by `node/src/runtime/tests.rs`.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `runtime/reconnecting_sync.rs::run_reconnecting_verified_sync_service*` family | upstream `Ouroboros.Consensus.Node.Run.runWith` reconnect loop — high-level orchestration that wires bootstrap → sync → reconnect cycles |
| `runtime/reconnecting_sync.rs::recover_ledger_state_for_runtime` | upstream `Ouroboros.Consensus.Storage.LedgerDB.OnDisk.initLedgerDB` runtime-side recovery wrapper |

### Cross-module dependencies (no promotions needed)

The cluster reaches into `runtime.rs`-private items via three
`use super::{...}` blocks — descendants-see-private-ancestors rule, no
`pub(super)` promotions on anything that wasn't already exposed:

- **15 private fns** (the largest single super:: surface in R271):
  `bootstrap_with_attempt_state`, `handle_reconnect_batch_error`,
  `preferred_hot_peer_handoff_target`, `prepare_reconnect_attempt_state`,
  `reconnect_storage_tip`, `refresh_chain_db_reconnect_fallback_peers`,
  `registry_reserve_bootstrap_attempt_peers`, `seed_chain_state_via_chain_db`,
  `shared_chaindb_lock_error`, `synchronize_chain_sync_to_point`,
  `trace_checkpoint_outcome`, `trace_epoch_boundary_events`,
  `trace_reconnectable_sync_error`, `trace_session_established`,
  `trace_shutdown_before_bootstrap`, `trace_shutdown_during_session`,
  plus `update_bp_state_nonce` and `update_bp_state_sigma`.
- **`super::ChainDbVolatileAccess`** — the trait runtime.rs defines for
  the `Arc<RwLock<ChainDb<...>>>::best_tip()` extension method. Pulled
  into the new module's namespace so the shared-ChainDb code paths
  compile.
- Items from `super::reconnecting::{...}` (R271j extraction) and
  `super::peer_session::{...}` (R271f extraction) and
  `super::keep_alive::{KeepAliveScheduler, trace_verified_sync_batch_applied}`
  (R271h extraction) — all reached via their existing `pub(super)`
  surfaces.

### Visibility / dependency fixups

1. **`type CheckpointPersistenceOutcome = LedgerCheckpointUpdateOutcome;`** —
   the type alias was originally defined in runtime.rs at line 2000 (outside
   the cluster). Re-defined locally in reconnecting_sync.rs since the
   cluster body uses it as a discriminant in `if let
   CheckpointPersistenceOutcome::Persisted { slot, .. }` matches.
2. **`RuntimeLedgerRecovery` struct fields promoted to `pub(crate)`** —
   the struct is the return type of the now-`pub(crate)`
   `recover_ledger_state_for_runtime` fn. Its three fields (`outcome`,
   `stake_snapshots`, `pool_block_counts`) are accessed by
   `node/src/runtime/tests.rs` via `super::recover_ledger_state_for_runtime(...)`,
   so they need to be at least `pub(crate)`.
3. **runtime.rs imports trimmed** — 12 items dropped from
   `use crate::sync::{...}` (the verified-sync-only names),
   `Future` from `std::future`, `StakeSnapshots` and `TxId` from
   `yggdrasil_ledger`, 11 names from `use reconnecting::{...}`, plus
   `KeepAliveScheduler` and `trace_verified_sync_batch_applied` from
   `keep_alive`. Two reconnecting names (`re_admit_rolled_back_tx_ids`,
   `record_verified_batch_progress`) and `VerifiedSyncServiceConfig` are
   used only by tests.rs and now gated with `#[cfg(test)]`.
4. **`runtime/tests.rs` super::* imports preserved** — all 38 names
   the test file imports via the `use super::{...}` block still resolve
   (most are still private fns in runtime.rs; the moved ones go through
   the new `pub use reconnecting_sync::{...}` re-export block).

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 4,155 | 2,163 | −1,992 |
| `node/src/runtime/reconnecting_sync.rs` | (new) | 2,065 | +2,065 |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

### Cumulative R271 progress

| Slice | File created | Lines moved | runtime.rs running size |
|---|---|---|---|
| R271a (RuntimeGovernorConfig) | `runtime/governor_config.rs` (191) | 168 | 7,101 |
| R271b (Block-producer config + state) | `runtime/block_producer_config.rs` (109) | 81 | 7,020 |
| R271c (LedgerJudgementSettings) | `runtime/ledger_judgement.rs` (45) | 25 | 6,995 |
| R271d (Mempool helpers) | `runtime/mempool_helpers.rs` (240) | 218 | 6,777 |
| R271e (TxSubmission service) | `runtime/tx_submission_service.rs` (273) | 234 | 6,543 |
| R271f (NodeConfig + PeerSession + sync-request) | `runtime/peer_session.rs` (421) | 377 | 6,166 |
| R271g (Bootstrap entry points) | `runtime/bootstrap.rs` (188) | 158 | 6,008 |
| R271h (KeepAliveScheduler + trace helpers) | `runtime/keep_alive.rs` (~100) | 74 | 5,934 |
| R271i revised (Trace-field builders) | `runtime/tracing.rs` (92) | 55 | 5,879 |
| R271j (ReconnectingRunState cluster) | `runtime/reconnecting.rs` (503) | 435 | 5,444 |
| R271k (Block-producer slot loop) | `runtime/block_producer_loop.rs` (503) | 465 | 4,979 |
| R271l (Governor loop) | `runtime/governor_loop.rs` (872) | 824 | 4,155 |
| **R271m (Reconnecting verified-sync family)** | **`runtime/reconnecting_sync.rs` (2,065)** | **1,992** | **2,163** |

Net `runtime.rs` reduction: **7,269 → 2,163 lines (−5,106, ~70 %)**.
**runtime.rs is now under 2,200 lines for the first time, down from the
~7,300-line monolith at session start.**

### Stop point — R271n is the residual cleanup slice

R271n will tackle the residual 2,163 lines in runtime.rs. Inventory:

- ~14 private trace / control / topology / ledger-snapshot helper fns
  (`trace_shutdown_*`, `synchronize_chain_sync_to_point`, the cluster of
  `apply_control_*` and `apply_*_weights` fns, `seed_peer_registry`,
  `local_root_targets_from_*`, `point_slot`, etc.).
- ~6 reconnect-attempt orchestration fns
  (`reconnect_preferred_peer*`, `prepare_reconnect_attempt_state`,
  `ordered_reconnect_fallback_peers`,
  `refresh_chain_db_reconnect_fallback_peers`).
- Connection-manager / governor-action plumbing
  (`apply_cm_actions`, `retire_failed_outbound_peer`,
  `governor_action_*`, `update_registry_status_from_cm`,
  `peer_status_from_cm_state`).
- Forge-side helpers (`tip_context_from_chain_db`,
  `mempool_entries_for_forging`, `self_validate_forged_block`,
  `kes_expiry_warning*`, `block_producer_ledger_state_judgement`).
- Trait + module declarations (`ChainDbVolatileAccess`,
  the mod / pub use re-export block).

Strategy: split into 3–4 sub-modules along functional clusters
(`runtime/control_bundle.rs`, `runtime/peer_management.rs`,
`runtime/forge_helpers.rs`, `runtime/cm_actions.rs`). Goal: bring
runtime.rs under ~500 lines as a thin orchestration shell holding
only `pub use` re-exports + crate-level docstring + `pub mod`
declarations.

After R271n, the runtime split is structurally complete.
The next big monolith is `node/src/sync.rs` at 9,567 lines (R271o+).

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271l closure: `2026-05-07-round-271l-runtime-governor-loop-extraction.md`
- Upstream Run.runWith reconnect loop:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Node/Run.hs`
- Upstream LedgerDB initialization:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/LedgerDB.hs`
