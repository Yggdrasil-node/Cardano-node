## Round 273e — `mempool/tx_state.rs` split into `tx_state/{state,shared}.rs`

Date: 2026-05-08
Branch: main
Type: Filename-mirror refactor (Phase γ R273 fifth slice — consensus crate sub-mirrors)

### Slice scope

Split `crates/consensus/src/mempool/tx_state.rs` (768 lines) into a
319-line parent `tx_state.rs` shell + two new sub-modules:

- `crates/consensus/src/mempool/tx_state/state.rs` (337 lines):
  `TxState` + `PeerTxState` + `FilterOutcome` + impls.
- `crates/consensus/src/mempool/tx_state/shared.rs` (157 lines):
  `SharedTxState` `Arc<RwLock<TxState>>` wrapper + impls.

The residual `tx_state.rs` keeps the module-level docstring,
`SizeInBytes` type alias, the `DEFAULT_KNOWN_CAPACITY` constant
(promoted to `pub(super)` for child access), `pub mod` declarations,
`pub use` re-exports, and the unchanged `#[cfg(test)] mod tests` block.

### Content distribution

**`tx_state/state.rs`** — mirrors upstream
`Ouroboros.Network.TxSubmission.Inbound.V2.State::SharedTxState` /
`PeerTxState` (the per-peer + global tracking that the inbound
TxSubmission2 mini-protocol uses to deduplicate TxIds across peers
and prevent double-fetches):

- `pub struct PeerTxState` — per-peer entry: `unacknowledged`,
  `in_flight`, `in_flight_sizes` map, `inflight_bytes` total.
- `pub struct FilterOutcome` — `to_fetch` + `already_known`.
- `pub struct TxState` (`peers` field promoted to `pub(super)` so
  shared.rs can read it for diagnostics) — global state: bounded
  `known` ring, FIFO eviction queue, global in-flight set,
  `inflight_bytes_total`, per-peer map.
- `impl Default for TxState` + `impl TxState` — `new`,
  `register_peer`, `unregister_peer`, `filter_advertised`,
  `mark_in_flight`, `mark_in_flight_sized`, `mark_received`,
  `mark_not_found`, `mark_confirmed`, plus the diagnostic accessors
  (`is_known`, `is_in_flight`, `peer_count`, `known_count`,
  `peer_inflight_bytes`, `inflight_bytes_total`).

**`tx_state/shared.rs`** — mirrors the runtime-facing handle the
inbound TxSubmission2 mini-protocol clients hold. Cloned handles share
the same underlying state through `Arc<RwLock<>>`:

- `pub struct SharedTxState` — `Arc<RwLock<TxState>>` wrapper.
- `impl SharedTxState` — `with_capacity`, `register_peer`,
  `unregister_peer`, `filter_advertised`, `mark_in_flight`,
  `mark_in_flight_sized`, `mark_received`, `mark_not_found`,
  `mark_confirmed`, plus the read-side diagnostics.
- `impl Default for SharedTxState`.

**`tx_state.rs`** (residual):

- Module-level docstring.
- `pub(super) const DEFAULT_KNOWN_CAPACITY: usize = 16_384;`
- `pub type SizeInBytes = u32;`
- `pub mod state; pub mod shared;`
- `pub use state::{FilterOutcome, PeerTxState, TxState};`
- `pub use shared::SharedTxState;`
- `#[cfg(test)] mod tests` — moved unchanged; imports updated to
  pull `SocketAddr` and `TxId` explicitly since the file-level
  `use` blocks moved into the sub-modules.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `crates/consensus/src/mempool/tx_state.rs` (shell) | `Ouroboros.Network.TxSubmission.Inbound.V2.State` (top-level) |
| `crates/consensus/src/mempool/tx_state/state.rs` | `Ouroboros.Network.TxSubmission.Inbound.V2.State::PeerTxState` + `SharedTxState` (data + sync impl) |
| `crates/consensus/src/mempool/tx_state/shared.rs` | `Ouroboros.Network.TxSubmission.Inbound.V2.State` STM-wrapped runtime handle |

### Cross-module dependencies

- `SizeInBytes` and `DEFAULT_KNOWN_CAPACITY` stay in tx_state.rs (the
  alias is `pub` for crate consumers; the const is `pub(super)` so
  state.rs can see it without re-export pollution).
- shared.rs reaches state.rs via
  `use super::{FilterOutcome, SizeInBytes, TxState};` (R273-pattern
  with the parent module hosting the cross-cluster types and the
  sub-modules cross-referencing through `super::`).
- The `peers` field of `TxState` promoted to `pub(super)` for tests
  that inspect peer state directly.

### Visibility / dependency fixups

1. **Test imports** — `tx_state.rs::tests::use super::*;` previously
   transitively brought in `SocketAddr` and `TxId` via the file-level
   `use` blocks. After extraction the tests now import them
   explicitly via `use std::net::SocketAddr;` and
   `use yggdrasil_ledger::TxId;`.
2. **Orphaned `#[derive]` fragment at end of state.rs** — the
   awk extract carried a 4-line trailing fragment of `SharedTxState`'s
   doc comment + `#[derive(Clone, Debug, Default)]` past the cut
   boundary. Removed inline.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/consensus/src/mempool/tx_state.rs` | 768 | 319 | −449 |
| `crates/consensus/src/mempool/tx_state/state.rs` | (new) | 337 | +337 |
| `crates/consensus/src/mempool/tx_state/shared.rs` | (new) | 157 | +157 |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

### Cumulative R273 progress

| Slice | Files moved/created | Source file Δ |
|---|---|---|
| R273a (praos) | `praos/{vrf,common}.rs` | 793 → 464 (−329) |
| R273b (nonce) | `nonce/{derivation,evolution}.rs` | 832 → 448 (−384) |
| R273c (opcert) | `opcert/{cert,counter}.rs` | 856 → 547 (−309) |
| R273d (mempool/queue) | `queue/{inner,shared}.rs` | 1,665 → 731 (−934) |
| **R273e (mempool/tx_state)** | **`tx_state/{state,shared}.rs`** | **768 → 319 (−449)** |

Total moved: 2,405 lines across 10 sub-modules.

### Stop point — R273f is the next slice

Remaining ≥600-line consensus files:

| File | Lines | Likely split |
|---|---|---|
| `crates/consensus/src/diffusion_pipelining.rs` | 747 | per-state-machine slice |
| `crates/consensus/src/chain_state.rs` | 654 | volatile / immutable / dep-state split |

Plus the plutus crate's >1k-line monoliths (`cost_model.rs` 1,718,
`types.rs` 1,707, `builtins.rs` 1,483, `machine.rs` 1,460, `flat.rs`
1,245) and crypto (`vrf.rs` 1,254, `sum_kes.rs` 1,018, `kes.rs` 939).

R273f candidate: `diffusion_pipelining.rs` (747 lines). It contains
the diffusion pipelining state machine that mirrors upstream
`Ouroboros.Consensus.Storage.ChainDB.Impl.Iterator` /
`Ouroboros.Consensus.Block.SupportsDiffusionPipelining`.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R273
- R273d closure: `2026-05-08-round-273d-mempool-queue-split.md`
- Upstream TxSubmission Inbound state:
  `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/src/Ouroboros/Network/TxSubmission/Inbound/V2/State.hs`
- Upstream TxSubmission Inbound decision:
  `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/src/Ouroboros/Network/TxSubmission/Inbound/V2/Decision.hs`
