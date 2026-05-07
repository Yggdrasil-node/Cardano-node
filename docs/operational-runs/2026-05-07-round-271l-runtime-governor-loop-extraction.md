## Round 271l — `runtime.rs` per-domain split: twelfth slice (Governor loop)

Date: 2026-05-07
Branch: main
Type: Filename-mirror refactor (Phase γ R271 twelfth slice — second big async-fn extraction)

### Slice scope

Extracted 824 source lines from `runtime.rs` into a new
`node/src/runtime/governor_loop.rs` (872 lines including module
docstring + imports). One item moved:

- `pub async fn run_governor_loop<I, V, L>(...)` — the long-lived
  peer-selection governor task that drives `GovernorState`,
  applies `GovernorAction`s through the connection manager / peer
  registry, refreshes ledger-peer sources, and emits the governor
  trace fields used by the metrics surface.

`runtime.rs` keeps a `pub mod governor_loop;` declaration plus a
`pub use governor_loop::run_governor_loop;` re-export so
`run_node.rs` and the runtime entry points continue to resolve
`crate::runtime::run_governor_loop` unchanged.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `runtime/governor_loop.rs::run_governor_loop` | upstream `Ouroboros.Network.PeerSelection.Governor.peerSelectionGovernor` (long-lived governor loop driving `PeerSelectionActions` decisions over the registry/connection-manager) |

### Cross-module dependencies (no promotions needed)

The function calls a cluster of private helpers that stay in
`runtime.rs`, all reached via a single `use super::{...};` block in
the new module:

- `super::OutboundPeerManager`
- `super::RuntimeRootPeerSources`
- `super::apply_cm_actions`
- `super::apply_control_close`
- `super::governor_action_name`
- `super::governor_action_peer`
- `super::outbound_cm_local_addr`
- `super::peer_share_request_amount`
- `super::refresh_ledger_peer_sources_from_chain_db`
- `super::reserve_bootstrap_sync_peers`
- `super::retire_failed_outbound_peer`
- `super::split_timeout_cm_actions_for_governor`
- `super::suppress_outbound_promotions_while_bootstrap_pending`
- `super::update_registry_status_from_cm`

All accessed via explicit `use super::{...};` — no `pub(super)`
promotions needed. The descendants-see-private-ancestors rule lets
the child module reference its parent's private items via `super::`.
R271l is the largest single confirmation of this pattern (14
cross-references vs R271k's 5).

### Visibility / dependency fixups

1. **`yggdrasil_network::*` re-imports.** The governor loop transitively
   uses 13 names from `yggdrasil_network` (`AcquireOutboundResult`,
   `ConsensusMode`, `NodePeerSharing`, `PeerSelectionCounters`,
   `PeerSelectionTimeouts`, `PeerStateAction`, `ReleaseOutboundResult`,
   `churn_mode_from_fetch_mode`, `compute_association_mode`,
   `fetch_mode_from_judgement`, `governor_action_to_peer_state_action`,
   `peer_selection_mode`, `pick_churn_regime`) that runtime.rs no longer
   references after the extraction. Trimmed those out of runtime.rs's
   `use yggdrasil_network::{...}` block; they now live only in
   `runtime/governor_loop.rs`. The names not imported there
   (`AbstractState`, `AfterSlot`, `BlockFetchClient`, `ChainSyncClient`,
   `CmAction`, `ConnectionManagerState`, `ConsensusLedgerPeerInputs`,
   `ConsensusLedgerPeerSource`, `ControlMessage`, etc.) stay in
   runtime.rs because the residual file (verified-sync entries +
   reconnecting helpers + plumbing fns) still uses them.
2. **`crate::tracer::{NodeMetrics, NodeTracer, trace_fields}`** — direct
   import in the new module so the governor loop can keep emitting
   `peer_selection_state` / `governor_action` / `connmgr_state` traces
   into the metrics surface.
3. **`super::governor_config::RuntimeGovernorConfig`** + **`super::peer_session::NodeConfig`** — explicit re-import at the
   sub-module level since both are siblings of `governor_loop`.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 4,979 | 4,155 | −824 |
| `node/src/runtime/governor_loop.rs` | (new) | 872 | +872 |

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
| **R271l (Governor loop)** | **`runtime/governor_loop.rs` (872)** | **824** | **4,155** |

Net `runtime.rs` reduction: **7,269 → 4,155 lines (−3,114, ~43 %)**.
**runtime.rs is now under 4,200 lines for the first time.**

### Stop point — R271m (`run_reconnecting_verified_sync_service` family) is the next slice

R271m will tackle the 4 `run_reconnecting_verified_sync_service*`
entry points (~3,000 lines of remaining 4,155 in runtime.rs).
Strategy: try a single-module bundle in `runtime/reconnecting_sync.rs`
first; if the parent-private dependency surface threshold rule
(R271i lesson — > ~6 promotions means extract a shared dependency
prelude first) flags it as too coupled, split into per-entry-point
sub-modules.

After R271m + R271n (residual cleanup) the runtime split is
functionally complete and runtime.rs should land under ~500 lines —
just public re-exports + crate-level docstring + module declarations.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271k closure: `2026-05-07-round-271k-runtime-block-producer-loop-extraction.md`
- Upstream peerSelectionGovernor:
  `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/src/Ouroboros/Network/PeerSelection/Governor.hs`
- Upstream governor decision modules:
  `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/src/Ouroboros/Network/PeerSelection/Governor/{ActivePeers,KnownPeers,EstablishedPeers,RootPeers,Monitor}.hs`
