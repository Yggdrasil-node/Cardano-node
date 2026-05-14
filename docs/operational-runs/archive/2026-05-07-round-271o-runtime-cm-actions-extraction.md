## Round 271o — `runtime.rs` per-domain split: fifteenth slice (Connection-manager actions)

Date: 2026-05-07
Branch: main
Type: Filename-mirror refactor (Phase γ R271 fifteenth slice — cm-actions cluster)

### Slice scope

Extracted **310 source lines** (11 fns) from `runtime.rs` into a new
`node/src/runtime/cm_actions.rs` (352 lines including module
docstring + imports). Items moved:

- `pub(super) fn governor_action_name`,
  `pub(super) fn governor_action_peer` — trace-fields / target-peer
  helpers for `GovernorAction` enum.
- `pub(super) fn direct_sync_bootstrap_pending`,
  `pub(super) fn suppress_outbound_promotions_while_bootstrap_pending`
  — bootstrap-coordination helpers.
- `pub(super) fn outbound_cm_local_addr`,
  `pub(super) fn data_flow_from_version_data`,
  `pub(super) fn peer_status_from_cm_state`,
  `pub(super) fn update_registry_status_from_cm` — per-peer mux registry
  state mapping helpers.
- `pub(super) async fn retire_failed_outbound_peer` — failed-peer
  cleanup that demotes through the connection manager + unregisters mux.
- `pub(super) async fn apply_cm_actions` — async dispatch of a batch of
  `CmAction`s.
- `pub(super) fn split_timeout_cm_actions_for_governor` — partitions
  timeout-driven `CmAction`s by governor-known peer membership.

`runtime.rs` keeps a `pub mod cm_actions;` declaration plus a
`use cm_actions::{...};` block bringing the 8 fns into runtime.rs's
namespace + a `#[cfg(test)] use cm_actions::direct_sync_bootstrap_pending;`
gate (only consumed by `node/src/runtime/tests.rs`).

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `runtime/cm_actions.rs::apply_cm_actions` | upstream `Ouroboros.Network.ConnectionManager.Core::ConnectionHandler` action dispatch |
| `runtime/cm_actions.rs::governor_action_name` / `_peer` | upstream `Ouroboros.Network.PeerSelection.Governor::PeerSelectionAction` discriminants |
| `runtime/cm_actions.rs::peer_status_from_cm_state` / `update_registry_status_from_cm` | upstream `Ouroboros.Network.ConnectionManager.Types::AbstractState` ↔ peer-registry state mapping |

### Cross-module dependencies

- All 11 fns promoted to `pub(super)` for sibling-module access
  (`governor_loop.rs`, `reconnecting_sync.rs`, `tests.rs` — same
  pattern as R271n).
- The cluster reaches outside via two `use super::{...};` blocks:
  `super::peer_management::OutboundPeerManager` (R271n surface) and
  `super::peer_session::NodeConfig` (R271f surface).
- Zero cross-cluster super:: refs to runtime.rs-private items.

### Visibility / dependency fixups

1. **runtime.rs imports trimmed** — removed `NodeMetrics` from
   `crate::tracer::*`, plus 9 names from `yggdrasil_network::*`
   (`AbstractState`, `CmAction`, `ConnectionManagerState`, `DataFlow`,
   `GovernorAction`, `GovernorState`, `NodeToNodeVersionData`,
   `PeerSource`, `PeerStatus`) that the cluster consumed.
2. **`direct_sync_bootstrap_pending` cfg-gated** — the fn is consumed
   only by `tests.rs`; runtime.rs's import is `#[cfg(test)]` to keep
   the lib build warning-clean.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 1,306 | 1,002 | −304 |
| `node/src/runtime/cm_actions.rs` | (new) | 352 | +352 |

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
| R271a–m | (12 slices) | 5,106 | 2,163 |
| R271n (Peer-management cluster) | `runtime/peer_management.rs` | 857 | 1,306 |
| **R271o (CM-actions cluster)** | **`runtime/cm_actions.rs`** | **304** | **1,002** |

Net `runtime.rs` reduction: **7,269 → 1,002 lines (−6,267, ~86 %)**.
**runtime.rs is now under 1,005 lines** for the first time.

### Stop point — R271p is the next residual cleanup slice

Remaining ~1,002 lines in runtime.rs cluster into ~5 logical groups
(line ranges relative to current runtime.rs):

- **Forge / KES helpers** (~130 lines, ll. 80-199): `tip_context_from_chain_db`,
  `mempool_entries_for_forging`, `extract_inner_block_bytes`,
  `self_validate_forged_block`, `KesExpiryWarning` struct + 2 fns.
- **Ledger-judgement helpers** (~175 lines, ll. 209-381):
  `ChainDbConsensusLedgerSource` struct + impl,
  `derive_judgement_for_observe`, `wall_clock_unix_secs`,
  `block_producer_ledger_state_judgement`, `FilePeerSnapshotSource`
  struct + impl.
- **Sync-session helpers + reconnect error handler + chain-db refresh**
  (~400 lines, ll. 506-846):
  `shared_chaindb_lock_error`, 3 `trace_*` shutdown/session helpers,
  `synchronize_chain_sync_to_point`, `trace_reconnectable_sync_error`,
  `handle_reconnect_batch_error`, `extend_unique_socket_addrs`,
  `refresh_chain_db_reconnect_fallback_peers`.
- **Checkpoint + epoch-boundary tracing** (~120 lines):
  `checkpoint_trace_fields`, `trace_checkpoint_outcome`,
  `trace_epoch_boundary_events`.
- **`refresh_ledger_peer_sources_from_chain_db`** (~62 lines).
- **ChainDb access trait** (~50 lines): `seed_chain_state_via_chain_db`,
  `trait ChainDbVolatileAccess`.

R271p will likely extract the **sync-session + checkpoint-tracing
clusters** as one combined module (`runtime/sync_session_helpers.rs`).
After R271p + R271q, runtime.rs lands at the planned ~500 lines.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271n closure: `2026-05-07-round-271n-runtime-peer-management-extraction.md`
- Upstream Connection Manager Core:
  `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network-framework/src/Ouroboros/Network/ConnectionManager/Core.hs`
- Upstream PeerSelection.Governor:
  `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/src/Ouroboros/Network/PeerSelection/Governor.hs`
