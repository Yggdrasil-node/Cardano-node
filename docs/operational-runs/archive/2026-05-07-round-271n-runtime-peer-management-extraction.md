## Round 271n — `runtime.rs` per-domain split: fourteenth slice (Peer-management cluster)

Date: 2026-05-07
Branch: main
Type: Filename-mirror refactor (Phase γ R271 fourteenth slice — peer-management cluster)

### Slice scope

Extracted **857 source lines** from `runtime.rs` into a new
`node/src/runtime/peer_management.rs` (923 lines including module
docstring + imports). Items moved:

- `pub(super) struct ManagedWarmPeer` + impl (per-peer warm state:
  session, keepalive cookie, hot/warm flag, last-known tip, mux
  temperature control bundle).
- `pub(super) struct OutboundPeerManager` + impl (governor-side cluster
  of warm peers + shared per-peer BlockFetch worker pool — 16 methods).
- `pub(super) struct RuntimeRootPeerSources` + impl (DNS root peer
  source bundle: local roots, bootstrap peers, public-config peers,
  ledger-peer source).
- 5 control-bundle helpers (`control_bundle_cold_to_warm`,
  `apply_control_activate/deactivate/close`, `apply_hot_weights`,
  `apply_warm_weights`).
- `pub type SharedFetchWorkerPool` + `pub fn new_shared_fetch_worker_pool`
  (kept public for lib.rs re-export).
- ~17 helper fns: `peer_share_request_amount`, `trace_root_refresh_error`,
  `seed_peer_registry`, `reserve_bootstrap_sync_peers`,
  `registry_reserve_bootstrap_attempt_peers`, `reconnect_storage_tip`,
  `local_root_targets_from_config`, `local_root_targets_from_resolved_groups`,
  `point_slot`, `preferred_hot_peer_from_registry`,
  `preferred_hot_peer_handoff_target`, `reconnect_preferred_peer_with_source`,
  `ordered_reconnect_fallback_peers`, `prepare_reconnect_attempt_state`,
  `reconnect_preferred_peer` (test-only), `extend_unique_peers`,
  `extend_unique_ledger_peers`, `ledger_peer_snapshot_from_ledger_state`.

`runtime.rs` keeps a `pub mod peer_management;` declaration plus three
re-export blocks:

- `pub use peer_management::{SharedFetchWorkerPool, local_root_targets_from_config, new_shared_fetch_worker_pool, seed_peer_registry};`
  (the public surface still re-exported by `node/src/lib.rs`).
- `use peer_management::{...};` (the 11 `pub(super)` items used by
  runtime.rs's residual fns and the other sub-modules under `runtime/`).
- `#[cfg(test)] use peer_management::{...};` (5 test-only items
  referenced by `node/src/runtime/tests.rs`).

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `runtime/peer_management.rs::ManagedWarmPeer` | upstream `Ouroboros.Network.PeerSelection.PeerStateActions::PeerStateActions` per-peer mux state |
| `runtime/peer_management.rs::OutboundPeerManager` | upstream `Ouroboros.Network.PeerSelection.Governor` warm-peer cluster + `Ouroboros.Network.BlockFetch.ClientRegistry` per-peer fetch state |
| `runtime/peer_management.rs::RuntimeRootPeerSources` | upstream `Ouroboros.Network.RootPeers` DNS root peer source bundle |

### Cross-module dependencies (R271i lesson applied)

The peer-management cluster crossed the R271i threshold (>~6 promotions
needed) because `OutboundPeerManager`, `RuntimeRootPeerSources`,
`ManagedWarmPeer` are all consumed by sibling modules (`governor_loop.rs`,
`reconnecting_sync.rs`, `tests.rs`). Per the threshold rule, a clean
extraction needed the structs + their cross-module surface promoted to
`pub(super)` rather than relying on descendants-see-private-ancestors.

Promoted to `pub(super)`:

- 3 structs (`ManagedWarmPeer`, `OutboundPeerManager`,
  `RuntimeRootPeerSources`) plus 8 fields on them.
- 22 methods total (4 on `ManagedWarmPeer`, 15 on `OutboundPeerManager`,
  3 on `RuntimeRootPeerSources`).
- 13 free fns used by sibling modules.
- 1 `pub(super)` re-export of `super::bootstrap::bootstrap` so the
  cluster's reconnect path can still call into `super::bootstrap_with_*`.

The cluster reaches outside via two `use super::{...};` blocks for
`bootstrap` and `peer_session::{NodeConfig, PeerSession}` — both
already-public surfaces of earlier rounds. Zero descendants-see-
ancestors private accesses needed.

### Visibility / dependency fixups

1. **`reconnect_preferred_peer` gated `#[cfg(test)]`** — the fn is
   defined `#[cfg(test)]` in the new module since it's only consumed by
   `tests.rs`. The runtime.rs re-export is matched with the same gate.
2. **`ManagedWarmPeer`, `ordered_reconnect_fallback_peers`,
   `preferred_hot_peer_from_registry`,
   `reconnect_preferred_peer_with_source`** — used only by tests.rs,
   gated `#[cfg(test)]` in runtime.rs's re-export block.
3. **runtime.rs imports trimmed** — dropped 1 item from `crate::sync::*`
   (kept `VerifiedSyncServiceConfig` `#[cfg(test)]` for tests.rs),
   `Duration`, `Instant`, `PoolRelayAccessPoint`, 18 names from
   `yggdrasil_network::*` (now used only in peer_management.rs). Two
   warn-clean import edits applied to keep the residual `runtime.rs`
   compile clean.
4. **Orphaned doc comment** — the cluster cut-off boundary (line 1064
   of pre-extraction runtime.rs) sat between `ledger_peer_snapshot_from_ledger_state`
   and the `ChainDbConsensusLedgerSource` struct's doc comment. The doc
   comment was carried forward into peer_management.rs by the bulk
   `awk` extract. Restored it inline at runtime.rs's
   `struct ChainDbConsensusLedgerSource` definition where it belongs;
   stripped from peer_management.rs.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 2,163 | 1,306 | −857 |
| `node/src/runtime/peer_management.rs` | (new) | 923 | +923 |

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
| R271a (RuntimeGovernorConfig) | `runtime/governor_config.rs` | 168 | 7,101 |
| R271b (Block-producer config + state) | `runtime/block_producer_config.rs` | 81 | 7,020 |
| R271c (LedgerJudgementSettings) | `runtime/ledger_judgement.rs` | 25 | 6,995 |
| R271d (Mempool helpers) | `runtime/mempool_helpers.rs` | 218 | 6,777 |
| R271e (TxSubmission service) | `runtime/tx_submission_service.rs` | 234 | 6,543 |
| R271f (NodeConfig + PeerSession + sync-request) | `runtime/peer_session.rs` | 377 | 6,166 |
| R271g (Bootstrap entry points) | `runtime/bootstrap.rs` | 158 | 6,008 |
| R271h (KeepAliveScheduler + trace helpers) | `runtime/keep_alive.rs` | 74 | 5,934 |
| R271i revised (Trace-field builders) | `runtime/tracing.rs` | 55 | 5,879 |
| R271j (ReconnectingRunState cluster) | `runtime/reconnecting.rs` | 435 | 5,444 |
| R271k (Block-producer slot loop) | `runtime/block_producer_loop.rs` | 465 | 4,979 |
| R271l (Governor loop) | `runtime/governor_loop.rs` | 824 | 4,155 |
| R271m (Reconnecting verified-sync family) | `runtime/reconnecting_sync.rs` | 1,992 | 2,163 |
| **R271n (Peer-management cluster)** | **`runtime/peer_management.rs`** | **857** | **1,306** |

Net `runtime.rs` reduction: **7,269 → 1,306 lines (−5,963, ~82 %)**.

### Stop point — R271o is the next residual cleanup slice

Remaining ~1,306 lines in runtime.rs cluster into ~6 logical groups:

- **Forge / KES helpers** (~135 lines): `tip_context_from_chain_db`,
  `mempool_entries_for_forging`, `extract_inner_block_bytes`,
  `self_validate_forged_block`, `KesExpiryWarning` + 2 helpers.
- **Ledger-judgement helpers** (~180 lines):
  `ChainDbConsensusLedgerSource` struct + impl,
  `derive_judgement_for_observe`, `wall_clock_unix_secs`,
  `block_producer_ledger_state_judgement`, `FilePeerSnapshotSource`
  struct + impl.
- **Connection-manager / governor-action plumbing** (~310 lines):
  `governor_action_name`, `governor_action_peer`,
  `direct_sync_bootstrap_pending`,
  `suppress_outbound_promotions_while_bootstrap_pending`,
  `outbound_cm_local_addr`, `data_flow_from_version_data`,
  `peer_status_from_cm_state`, `update_registry_status_from_cm`,
  `retire_failed_outbound_peer`, `apply_cm_actions`,
  `split_timeout_cm_actions_for_governor`.
- **Sync-session helpers + reconnect error handler + chain-db refresh**
  (~350 lines): `shared_chaindb_lock_error`, 3 `trace_*` shutdown/
  session helpers, `synchronize_chain_sync_to_point`,
  `trace_reconnectable_sync_error`, `handle_reconnect_batch_error`,
  `extend_unique_socket_addrs`, `refresh_chain_db_reconnect_fallback_peers`.
- **Checkpoint + epoch-boundary tracing** (~120 lines):
  `checkpoint_trace_fields`, `trace_checkpoint_outcome`,
  `trace_epoch_boundary_events`.
- **ChainDbVolatileAccess trait + helper** (~50 lines):
  `seed_chain_state_via_chain_db`, `trait ChainDbVolatileAccess`.
- **`refresh_ledger_peer_sources_from_chain_db`** (~62 lines): the
  ledger-peer source refresher orchestration.

R271o will extract 2-3 of these clusters in the same pattern.
After R271o + R271p the runtime split should be functionally complete
and runtime.rs should land under ~500 lines.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271m closure: `2026-05-07-round-271m-runtime-reconnecting-sync-extraction.md`
- Upstream peer-state actions:
  `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/src/Ouroboros/Network/PeerSelection/PeerStateActions.hs`
- Upstream root-peer providers:
  `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/src/Ouroboros/Network/RootPeers.hs`
- Upstream BlockFetch client registry:
  `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network-protocols/src/Ouroboros/Network/BlockFetch/ClientRegistry.hs`
