## Round 271r — `runtime.rs` per-domain split: eighteenth slice (Sync-session helpers)

Date: 2026-05-07
Branch: main
Type: Filename-mirror refactor (Phase γ R271 eighteenth slice — sync-session cluster)

### Slice scope

Extracted **438 source lines** (12 fns + 1 type alias) from `runtime.rs`
into a new `node/src/runtime/sync_session.rs` (495 lines including
module docstring + imports). Items moved:

- `pub(super) fn shared_chaindb_lock_error` — uniform recovery error
  for `Arc<RwLock<ChainDb>>` lock poisoning.
- `pub(super) fn trace_shutdown_before_bootstrap`,
  `pub(super) fn trace_shutdown_during_session`,
  `pub(super) fn trace_session_established` — lifecycle trace helpers.
- `pub(super) async fn synchronize_chain_sync_to_point` — typed-ChainSync
  `MsgFindIntersect` synchronisation around the locally-tracked chain
  point.
- `pub(super) fn trace_reconnectable_sync_error` — uniform reconnect
  trace.
- `pub(super) fn handle_reconnect_batch_error` — three-way
  Reconnect/Fail/Continue disposition for sync errors.
- `pub(super) fn extend_unique_socket_addrs` — uniqueness-preserving
  append helper.
- `pub(super) fn refresh_chain_db_reconnect_fallback_peers` — re-derives
  the reconnect fallback peer list from current ChainDb / topology /
  peer-snapshot state.
- `pub(super) type CheckpointPersistenceOutcome = LedgerCheckpointUpdateOutcome;`
  — re-exported alias.
- `pub(super) fn checkpoint_trace_fields`,
  `pub(super) fn trace_checkpoint_outcome`,
  `pub(super) fn trace_epoch_boundary_events` — checkpoint persistence +
  epoch-boundary trace event surface.

`runtime.rs` keeps a `pub mod sync_session;` declaration plus two
`use sync_session::{...};` blocks: a primary one bringing the 10 fns
into runtime.rs's namespace for residual fns, and a `#[cfg(test)]`
gate for the 2 names only consumed by `node/src/runtime/tests.rs`
(`CheckpointPersistenceOutcome` and `checkpoint_trace_fields`).

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `runtime/sync_session.rs::synchronize_chain_sync_to_point` | upstream `Ouroboros.Network.Protocol.ChainSync.Client::chainSyncClientPeer` MsgFindIntersect |
| `runtime/sync_session.rs::handle_reconnect_batch_error` | upstream `Ouroboros.Consensus.MiniProtocol.BlockFetch.ClientInterface::mkAddFetchedBlock_` peer-attribution path |
| `runtime/sync_session.rs::trace_checkpoint_outcome` / `trace_epoch_boundary_events` | upstream `Cardano.Node.Tracers::TraceLedgerEvent` checkpoint + epoch-boundary observability |

### Cross-module dependencies

- 11 fns + 1 type alias promoted to `pub(super)` for sibling consumers
  (`reconnecting_sync.rs`, `tests.rs`).
- The cluster reaches outside via four `use super::{...};` paths:
  - `super::keep_alive::trace_sync_failure` (R271h)
  - `super::reconnecting::BatchErrorDisposition` (R271j)
  - `super::tracing::{peer_point_trace_fields, session_established_trace_fields, sync_error_trace_fields}` (R271i)
- Plus `crate::config::load_peer_snapshot_file`,
  `crate::sync::{LedgerCheckpointTracking, LedgerCheckpointUpdateOutcome,
  SyncError, TypedIntersectResult, typed_find_intersect}`,
  `crate::tracer::{NodeTracer, trace_fields}`, multiple
  `yggdrasil_*` types — all already-public crate-level items.

### Visibility / dependency fixups

1. **Orphaned doc comment carried back** — the 7-line module-level
   doc comment for `seed_chain_state_via_chain_db` was carried into
   sync_session.rs by the bulk awk extract. Restored it inline at
   runtime.rs's residual `seed_chain_state_via_chain_db` definition.
2. **`mod reconnecting; mod tracing; mod keep_alive;` declarations
   stay in runtime.rs** — they define sibling sub-modules of
   `sync_session`, so they remain at the runtime.rs level and are
   reached by sync_session.rs via `super::keep_alive::*`,
   `super::reconnecting::*`, `super::tracing::*` imports.
3. **runtime.rs imports trimmed aggressively** — runtime.rs's import
   block dropped 9 names from `crate::*`, 4 names from `serde_json`,
   `BTreeMap`, `SocketAddr`, `Path`, `EpochBoundaryEvent`, 13 names
   from `yggdrasil_network::*`, `BatchErrorDisposition` (test-only),
   `peer_point_trace_fields`, `session_established_trace_fields`
   (test-only), `trace_sync_failure`, and `verified_sync_batch_trace_fields` (kept).
4. **`type CheckpointTracking` alias** — runtime.rs keeps a local
   `type CheckpointTracking = LedgerCheckpointTracking;` because
   `refresh_ledger_peer_sources_from_chain_db` and the residual
   `seed_chain_state_via_chain_db` still reference the alias.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 698 | 265 | −433 |
| `node/src/runtime/sync_session.rs` | (new) | 495 | +495 |

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
| R271a–q | (17 slices) | 6,571 | 698 |
| **R271r (Sync-session cluster)** | **`runtime/sync_session.rs`** | **433** | **265** |

Net `runtime.rs` reduction: **7,269 → 265 lines (−7,004, ~96.4 %)**.
**runtime.rs is now under 270 lines** — exceeded the planned
~500-line target.

### What remains in runtime.rs (~265 lines)

1. ~80 lines of `use` imports.
2. ~20 lines: `ChainTipNotify` type alias + module-level docstring.
3. ~20 sub-module declarations (`pub mod ...; pub use ...;`):
   `block_producer_config`, `governor_config`, `peer_management`,
   `cm_actions`, `forge`, `ledger_judgement`, `ledger_peer_source`,
   `block_producer_loop`, `governor_loop`, `mempool_helpers`,
   `tx_submission_service`, `peer_session`, `bootstrap`,
   `reconnecting_sync`, `sync_session`, `reconnecting`, `tracing`,
   `keep_alive`.
4. **`refresh_ledger_peer_sources_from_chain_db`** (~62 lines) — the
   ledger-peer-source refresher orchestration. Could fold into
   `runtime/ledger_peer_source.rs` in a follow-up round.
5. **`seed_chain_state_via_chain_db` + `trait ChainDbVolatileAccess`**
   (~50 lines) — ChainDb volatile-store access trait used by the
   reconnecting verified-sync inners. Could fold into
   `runtime/reconnecting_sync.rs` or stay as a thin runtime-level
   trait.
6. `type CheckpointTracking = LedgerCheckpointTracking;` (1 line).
7. `#[cfg(test)] mod tests;` (2 lines).

### Stop point — R271 split is functionally complete

R271 concludes here as the planned filename-mirror split for
runtime.rs. Optional follow-up rounds:

- **R271s** (optional): fold `refresh_ledger_peer_sources_from_chain_db`
  into `runtime/ledger_peer_source.rs` (the natural home).
- **R271t** (optional): fold `seed_chain_state_via_chain_db` +
  `ChainDbVolatileAccess` into `runtime/reconnecting_sync.rs` (the
  sole consumer).

After those two optional folds, runtime.rs would land at ~150 lines
of nothing but `use` imports, `pub mod ...; pub use ...;` re-export
blocks, the `ChainTipNotify` type alias, and the
`#[cfg(test)] mod tests;` declaration.

### Next R271 arc tasks (per the plan)

- R269r-style Conway-rule sub-mirror under `state/eras/conway/rules/`
  (~3 days, 19 substantive .rs files mirroring upstream).
- R272 pre-Conway era rules split (5 days).
- R273 consensus + plutus + crypto + storage submodule splits (3 days).

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271q closure: `2026-05-07-round-271q-runtime-ledger-peer-source-extraction.md`
- Upstream reconnect / shutdown lifecycle:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Node/Run.hs`
- Upstream typed ChainSync MsgFindIntersect:
  `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/ChainSync/Client.hs`
- Upstream BlockFetch peer attribution:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/MiniProtocol/BlockFetch/ClientInterface.hs`
