## Round 271q — `runtime.rs` per-domain split: seventeenth slice (Ledger-peer-source bridges)

Date: 2026-05-07
Branch: main
Type: Filename-mirror refactor (Phase γ R271 seventeenth slice)

### Slice scope

Extracted **186 source lines** from `runtime.rs` into a new
`node/src/runtime/ledger_peer_source.rs` (223 lines including
module docstring + imports). Items moved:

- `pub(super) struct ChainDbConsensusLedgerSource<'a, I, V, L>` (7
  fields, all `pub(super)`) + `impl ConsensusLedgerPeerSource for ...`
  — the consensus-fed ledger-peer source bridging `ChainDb` to the
  network crate's `live_refresh_ledger_peer_registry` orchestration.
- `pub(super) fn derive_judgement_for_observe` — derives a
  `LedgerStateJudgement` from the recovered tip's wall-clock age,
  falling back to `YoungEnough` when genesis timing inputs are missing.
- `pub(crate) fn derive_judgement_at` — pure variant that takes an
  explicit `now_unix_secs` for deterministic testing.
- `pub(super) fn wall_clock_unix_secs` — `SystemTime::now()` ↔
  Unix-epoch f64 helper.
- `pub(super) fn block_producer_ledger_state_judgement` — variant
  that reads `RuntimeBlockProducerConfig.max_ledger_state_age_secs`.
- `pub(super) struct FilePeerSnapshotSource<'a>` (2 fields, all
  `pub(super)`) + `impl PeerSnapshotFileSource for ...` — re-reads
  the configured `peerSnapshotFile` path each tick.

`runtime.rs` keeps a `pub mod ledger_peer_source;` declaration plus
two re-export blocks: a primary `use ledger_peer_source::{...};`
block bringing the 3 names runtime.rs's residual fns and other
sub-modules consume, and a `#[cfg(test)] use ledger_peer_source::{
derive_judgement_at, wall_clock_unix_secs};` gate (only consumed by
`node/src/runtime/tests.rs`).

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `runtime/ledger_peer_source.rs::ChainDbConsensusLedgerSource` | upstream `Ouroboros.Network.Diffusion.LedgerPeers::LedgerPeers` consensus-fed source |
| `runtime/ledger_peer_source.rs::derive_judgement_at` / `derive_judgement_for_observe` | upstream `Cardano.Node.Diffusion.Configuration::mkLedgerStateJudgement` |
| `runtime/ledger_peer_source.rs::FilePeerSnapshotSource` | upstream `Ouroboros.Network.PeerSelection.LedgerPeers::PeerSnapshot` file-source variant |

### Cross-module dependencies

- 5 fns + 2 structs + 9 fields promoted to `pub(super)` for sibling
  consumers (`block_producer_loop.rs` calls `block_producer_ledger_state_judgement`,
  `tests.rs` tests `derive_judgement_at` and `wall_clock_unix_secs`).
- The cluster reaches outside via four `use super::{...};` paths:
  - `super::block_producer_config::RuntimeBlockProducerConfig` (R271b)
  - `super::peer_management::{ledger_peer_snapshot_from_ledger_state, point_slot}` (R271n)
  - `crate::config::load_peer_snapshot_file`
  - `crate::sync::{recover_ledger_state_chaindb, recover_ledger_state_chaindb_epoch_boundary}`
- Zero `super::*` references to runtime.rs-private items.

### Visibility / dependency fixups

1. **Orphaned doc comment** — runtime.rs originally had a 14-line
   module-level doc comment for `ChainDbConsensusLedgerSource` sitting
   above the `pub mod ledger_judgement;` declaration. Removed when the
   struct moved; the module-doc above ledger_peer_source.rs covers
   the same concept inline.
2. **runtime.rs imports trimmed** — dropped
   `recover_ledger_state_chaindb`, `recover_ledger_state_chaindb_epoch_boundary`
   from `crate::sync::*`; `SlotNo` from `yggdrasil_ledger`;
   `ConsensusLedgerPeerInputs`, `ConsensusLedgerPeerSource`,
   `PeerSnapshotFileObservation`, `PeerSnapshotFileSource` from
   `yggdrasil_network`; `ledger_peer_snapshot_from_ledger_state`,
   `point_slot` from `peer_management::*`.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 884 | 698 | −186 |
| `node/src/runtime/ledger_peer_source.rs` | (new) | 223 | +223 |

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
| R271a–p | (16 slices) | 6,385 | 884 |
| **R271q (Ledger-peer-source bridges)** | **`runtime/ledger_peer_source.rs`** | **186** | **698** |

Net `runtime.rs` reduction: **7,269 → 698 lines (−6,571, ~90.4 %)**.
**runtime.rs is now under 700 lines** for the first time.

### Stop point — R271r is the next residual cleanup slice

Remaining ~698 lines in runtime.rs cluster into ~4 logical groups:

- **`refresh_ledger_peer_sources_from_chain_db`** (~62 lines) — the
  ledger-peer-source refresher orchestration. Could be folded into
  `runtime/ledger_peer_source.rs` (R271q) or stay where it sits.
- **Sync-session helpers + reconnect error handler + chain-db refresh**
  (~340 lines): `shared_chaindb_lock_error`, 3 `trace_*` shutdown/
  session helpers, `synchronize_chain_sync_to_point`,
  `trace_reconnectable_sync_error`, `handle_reconnect_batch_error`,
  `extend_unique_socket_addrs`,
  `refresh_chain_db_reconnect_fallback_peers`.
- **Checkpoint + epoch-boundary tracing** (~120 lines):
  `checkpoint_trace_fields`, `trace_checkpoint_outcome`,
  `trace_epoch_boundary_events`.
- **ChainDb access trait** (~50 lines): `seed_chain_state_via_chain_db`,
  `trait ChainDbVolatileAccess`.

Plus the `mod reconnecting; mod tracing; mod keep_alive;` declarations
and 8 `pub mod ...; pub use ...;` re-export blocks (~80 lines, won't
move).

R271r candidate: extract sync-session helpers + checkpoint tracing
(~460 lines) as a single `runtime/sync_session.rs`. After R271r the
runtime split is functionally complete and runtime.rs lands at
~250 lines (mod decls + re-exports + ChainDbVolatileAccess trait).

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271p closure: `2026-05-07-round-271p-runtime-forge-extraction.md`
- Upstream LedgerPeers (consensus source):
  `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/src/Ouroboros/Network/Diffusion/LedgerPeers.hs`
- Upstream mkLedgerStateJudgement:
  `.reference-haskell-cardano-node/cardano-node/src/Cardano/Node/Diffusion/Configuration.hs`
