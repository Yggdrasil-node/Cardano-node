## Round 271f — `runtime.rs` per-domain split: sixth slice (NodeConfig + PeerSession + verified-sync request types)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R271 sixth slice)

### Slice scope

Extracted 377 source lines from `runtime.rs` into a new
`node/src/runtime/peer_session.rs` (421 lines). This is the largest
single R271 extraction so far. Items moved (6 top-level types + 4
impl blocks):

**Connection bring-up:**
- `NodeConfig` — peer address, network magic, handshake versions,
  bootstrap topology subset.
- `PeerSession` — owned bundle of the 5 mini-protocol clients
  (ChainSync, BlockFetch, KeepAlive, TxSubmission, PeerSharing) +
  `ConnectionManagerState` + `AbstractState` sender.
- `impl PeerSession` (~50 lines: lifetime helpers and tear-down).

**Service-exit summaries:**
- `ReconnectingSyncServiceOutcome` (struct).
- `ResumedSyncServiceOutcome` (struct).

**Verified-sync entry-point requests:**
- `ReconnectingVerifiedSyncRequest<'a>` — builder for the cold-start
  reconnecting verified-sync entry point.
- `impl<'a> ReconnectingVerifiedSyncRequest<'a>` with the
  `with_*` builder methods for ~13 optional cross-task shared handles
  (nonce_state, peer_snapshot_path, metrics, peer_registry, mempool,
  tentative_state, tip_notify, bp_state, bp_pool_key_hash,
  inbound_tx_state, chain_dep_persist_dir, etc.).
- `ResumeReconnectingVerifiedSyncRequest<'a>` — companion builder for
  the resume-from-recovered-storage variant.
- `impl<'a> ResumeReconnectingVerifiedSyncRequest<'a>` with the same
  builder pattern.

`runtime.rs` keeps a `pub mod peer_session;` declaration plus a
`pub use peer_session::{…};` re-export listing all 6 top-level items
so existing callers (run_node.rs, validate_config.rs, the bootstrap
async fns later in runtime.rs) continue to resolve via
`crate::runtime::*` unchanged.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `runtime/peer_session.rs::NodeConfig` | upstream `Ouroboros.Network.NodeToNode` connection-config record + bootstrap subset |
| `runtime/peer_session.rs::PeerSession` | upstream `Ouroboros.Network.NodeToNode.PeerSession` mini-protocol client bundle |
| `runtime/peer_session.rs::ReconnectingVerifiedSyncRequest` / `ResumeReconnectingVerifiedSyncRequest` | yggdrasil-only — driver-struct shape for the verified-sync runtime entry points; closest upstream is `Ouroboros.Consensus.Node.Run.runWith` parameter bundle |
| `runtime/peer_session.rs::*Outcome` types | yggdrasil-only summary records for service-exit observability |

### Visibility / dependency fixups

The cluster pulls in a wide cross-section of types from 6 different
crates. Initial extraction missed several:

1. **`MiniProtocolNum`** (PeerSession's keepalive handle field) — needed
   from `yggdrasil_network`.
2. **`Point`** (in `ReconnectingVerifiedSyncRequest` recovery fields) —
   needed from `yggdrasil_ledger`.
3. **`ChainState`** (in service outcomes) — needed from
   `yggdrasil_consensus`.
4. **`LedgerRecoveryOutcome`** (in `ResumeReconnectingVerifiedSyncRequest`)
   — was originally imported as `yggdrasil_ledger::LedgerRecoveryOutcome`
   but actually lives at `yggdrasil_storage::LedgerRecoveryOutcome`
   (the chain_db crate). Path correction needed.
5. **Unused over-imports** trimmed: `AbstractState`, `AfterSlot`,
   `ConnectionManagerState`, `Path`, `PathBuf`, `NodeTracer` —
   speculatively imported but not actually referenced by the
   extracted bodies.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 6,543 | 6,166 | −377 |
| `node/src/runtime/peer_session.rs` | (new) | 421 | +421 |

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
| **R271f (NodeConfig + PeerSession + sync-request)** | **`runtime/peer_session.rs` (421)** | **377** | **6,166** |

Net `runtime.rs` reduction so far: **7,269 → 6,166 lines (−1,103, ~15 %)**.

### Stop point — R271g (big sync-session helpers) is the next slice

Remaining major candidates per `docs/REFACTOR_BLUEPRINT.md`:

| Round | Target | Approx lines |
|---|---|---|
| R271g | `bootstrap` / `bootstrap_with_fallbacks` async fns + `ReconnectingRunState` impl (sync-session bring-up) | ~600 |
| R271h | The big `run_governor_loop` + `run_block_producer_loop` async fns | ~1,500 |
| R271i+ | Reconnecting verified sync helpers (the rest of runtime.rs) | ~2,500 |
| R271j+ | sync.rs split (separate arc, 9,567 lines) | many slices |

R271f is a meaningful watershed — after it, what remains in runtime.rs
is mostly long async fn bodies that orchestrate sync sessions, the
governor loop, and the block-producer loop. Those orchestration fns
are conceptually different from the configuration/data-type slices
landed so far, and will require a different splitting strategy
(probably one fn-per-file for the largest async fns, or
multiple-fn-per-file for the helper families).

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271e closure: `2026-05-06-round-271e-runtime-tx-submission-service-extraction.md`
- Upstream NodeToNode connection config: `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/lib/Ouroboros/Network/NodeToNode.hs`
- Upstream verified-sync entry point: `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Node/Run.hs`
