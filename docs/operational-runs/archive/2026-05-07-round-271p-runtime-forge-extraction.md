## Round 271p — `runtime.rs` per-domain split: sixteenth slice (Forge / KES helpers)

Date: 2026-05-07
Branch: main
Type: Filename-mirror refactor (Phase γ R271 sixteenth slice — forge cluster)

### Slice scope

Extracted **120 source lines** from `runtime.rs` into a new
`node/src/runtime/forge.rs` (151 lines including module docstring +
imports). Items moved:

- `pub(super) fn tip_context_from_chain_db<I, V, L>` — derive
  `(SlotNo, BlockNo, HeaderHash)` triple from current ChainDb tip.
- `pub(super) fn mempool_entries_for_forging` — extract fee-ordered
  mempool slice for body assembly.
- `pub(super) fn extract_inner_block_bytes` — re-decode block envelope
  to recover the inner-block CBOR slice for body-size validation.
- `pub(super) fn self_validate_forged_block` — protocol-version,
  body-hash, body-size, header-hash, slot, and block-number sanity
  checks against a freshly-forged `ForgedBlock`.
- `pub(super) struct KesExpiryWarning` (5 fields, all `pub(super)`)
  + `pub(super) fn kes_expiry_warning` + `pub(super) fn
  kes_expiry_warning_from_periods` — KES expiry surveillance for
  operator observability.

`runtime.rs` keeps a `pub mod forge;` declaration plus a
`use forge::{...};` block bringing the 4 main fns into runtime.rs's
namespace. `kes_expiry_warning_from_periods` is gated `#[cfg(test)]`
since it's only consumed by `node/src/runtime/tests.rs`.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `runtime/forge.rs::tip_context_from_chain_db` | upstream `Cardano.Node.Forge::ForgeContext` tip lookup |
| `runtime/forge.rs::self_validate_forged_block` | upstream `Cardano.Node.Tracers::TraceForgeEvent::TraceForgedInvalidBlock` self-checks |
| `runtime/forge.rs::KesExpiryWarning` + `kes_expiry_warning*` | upstream `Cardano.Node.Forge::praosCheckCanForge` / `KESInfo` operator-observability around certificate validity |

### Cross-module dependencies

- 4 fns + 1 struct + 5 fields promoted to `pub(super)` for sibling
  consumers (`block_producer_loop.rs`, `tests.rs`).
- The cluster reaches outside via `crate::block_producer::*`,
  `crate::sync::*`, `yggdrasil_consensus::*`, `yggdrasil_ledger::*`,
  `yggdrasil_storage::*` — all already-public crate-level imports.
  Zero `super::*` references.

### Visibility / dependency fixups

1. **runtime.rs imports trimmed** — removed all of
   `use crate::block_producer::{BlockProducerCredentials, ForgedBlock,
   serialize_forged_block_cbor};`, 5 `crate::sync::*` items,
   `MEMPOOL_ZERO_IDX`, `MempoolEntry`, `SharedMempool` from
   consensus mempool, `kes_period_of_slot` from consensus, and
   `BlockNo`, `Decoder`, `HeaderHash` from `yggdrasil_ledger` — now
   used only in forge.rs.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 1,002 | 884 | −118 |
| `node/src/runtime/forge.rs` | (new) | 151 | +151 |

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
| R271o (CM-actions cluster) | `runtime/cm_actions.rs` | 304 | 1,002 |
| **R271p (Forge / KES helpers)** | **`runtime/forge.rs`** | **118** | **884** |

Net `runtime.rs` reduction: **7,269 → 884 lines (−6,385, ~88 %)**.
**runtime.rs is now under 900 lines** for the first time.

### Stop point — R271q is the next residual cleanup slice

Remaining ~884 lines in runtime.rs cluster into ~5 logical groups:

- **Ledger-judgement helpers** (~175 lines):
  `ChainDbConsensusLedgerSource` struct + impl,
  `derive_judgement_for_observe`, `wall_clock_unix_secs`,
  `block_producer_ledger_state_judgement`, `FilePeerSnapshotSource`
  struct + impl.
- **`refresh_ledger_peer_sources_from_chain_db`** (~62 lines).
- **Sync-session helpers + reconnect error handler + chain-db refresh**
  (~340 lines): `shared_chaindb_lock_error`, 3 `trace_*` shutdown/
  session helpers, `synchronize_chain_sync_to_point`,
  `trace_reconnectable_sync_error`, `handle_reconnect_batch_error`,
  `extend_unique_socket_addrs`, `refresh_chain_db_reconnect_fallback_peers`.
- **Checkpoint + epoch-boundary tracing** (~120 lines):
  `checkpoint_trace_fields`, `trace_checkpoint_outcome`,
  `trace_epoch_boundary_events`.
- **ChainDb access trait** (~50 lines): `seed_chain_state_via_chain_db`,
  `trait ChainDbVolatileAccess`.

Plus the `mod reconnecting; mod tracing; mod keep_alive;` declarations
and 8 `pub mod ...; pub use ...;` re-export blocks (~80 lines, won't
move).

R271q candidate: extract ledger-judgement helpers
(`ChainDbConsensusLedgerSource` + `FilePeerSnapshotSource` +
`derive_judgement_for_observe` + `block_producer_ledger_state_judgement`)
as `runtime/ledger_judgement_helpers.rs` (~175 lines).

After R271q + R271r + R271s the runtime split should be functionally
complete, runtime.rs landing at the planned ~500 lines (~80 lines of
mod decls + ~50 lines of trait + helper + ~50 lines of stragglers +
~20 lines of imports + tests cfg).

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271o closure: `2026-05-07-round-271o-runtime-cm-actions-extraction.md`
- Upstream forge module:
  `.reference-haskell-cardano-node/cardano-node/src/Cardano/Node/Forge.hs`
- Upstream `praosCheckCanForge`:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-protocol/src/Ouroboros/Consensus/Protocol/Praos.hs`
