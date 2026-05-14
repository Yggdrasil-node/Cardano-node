## Round 273d — `mempool/queue.rs` split into `queue/{inner,shared}.rs`

Date: 2026-05-08
Branch: main
Type: Filename-mirror refactor (Phase γ R273 fourth slice — consensus crate sub-mirrors)

### Slice scope

Split `crates/consensus/src/mempool/queue.rs` (1,665 lines) into a
731-line parent `queue.rs` shell + two new sub-modules:

- `crates/consensus/src/mempool/queue/inner.rs` (658 lines): `Mempool`
  fee-ordered queue policy struct + impls.
- `crates/consensus/src/mempool/queue/shared.rs` (324 lines):
  `SharedMempool` `Arc<RwLock<Mempool>>` wrapper + `Default` impl.

The residual `queue.rs` keeps the supporting types
(`MempoolEntry`, `MempoolError`, `MempoolRelayError`,
`IndexedMempoolEntry`, `MempoolSnapshot`, `TxSubmissionMempoolReader`,
`SharedTxSubmissionMempoolReader`), the `pub mod` declarations + `pub use`
re-exports for the moved types, and the unchanged tests.

### Content distribution

**`mempool/queue/inner.rs`** — mirrors upstream
`Ouroboros.Consensus.Mempool.API::Mempool` and the `Impl.Update`
insert / remove / purge logic:

- `pub struct Mempool` (4 fields, `max_bytes` `pub(super)` for
  `SharedMempool` access) — fee-ordered queue with capacity tracking
  and duplicate detection.
- `impl Mempool` — `with_capacity`, `insert`, `insert_with_eviction`,
  `insert_checked`, `insert_checked_with_eviction`, `pop_best`,
  `remove_by_id`, `remove_confirmed`, `remove_conflicting_inputs`,
  `purge_expired`, `revalidate`, `revalidate_against_protocol_params`,
  `snapshot`, `txid_set`, `len`, `is_empty`, plus the lower-level
  helpers (`evict_for_capacity`, `lower_fee_tail` etc).

**`mempool/queue/shared.rs`** — mirrors upstream
`Ouroboros.Consensus.Mempool` STM-wrapped API. The runtime-facing
handle wraps `Mempool` with `Arc<RwLock<>>` for concurrent access and
a `tokio::sync::Notify` so `LocalTxMonitor::AwaitAcquire` can block
until the mempool snapshot has changed:

- `pub struct SharedMempool` — 2 fields (private; only the `inner`
  Mempool is mutated through delegate methods).
- `impl SharedMempool` — `new`, `with_capacity`, `wait_for_change`,
  plus thin delegating wrappers around all `Mempool` mutators that
  `notify_waiters()` after a successful change.
- `impl Default for SharedMempool`.

**`queue.rs`** (residual) — supporting types + sub-mod decls + tests:

- All 7 supporting types (`MempoolEntry`, `MempoolRelayError`,
  `MempoolError`, `IndexedMempoolEntry`, `MempoolSnapshot`,
  `TxSubmissionMempoolReader`, `SharedTxSubmissionMempoolReader`)
  stay in queue.rs because they're cross-cluster — both inner.rs and
  shared.rs reference them via `super::`.
- `pub mod inner; pub mod shared;`
- `pub use inner::Mempool;` + `pub use shared::SharedMempool;`
- `#[cfg(test)] mod tests` block — moved unchanged.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `crates/consensus/src/mempool/queue.rs` (shell + types) | `Ouroboros.Consensus.Mempool.API` (top-level types) + `Ouroboros.Consensus.Mempool.TxSeq` (entry / error types) |
| `crates/consensus/src/mempool/queue/inner.rs` | `Ouroboros.Consensus.Mempool.Impl.Common` + `Impl.Update` (queue policy) |
| `crates/consensus/src/mempool/queue/shared.rs` | `Ouroboros.Consensus.Mempool` STM-wrapped API |

### Cross-module dependencies

- The 7 supporting types stay in queue.rs (parent) because both
  `inner.rs` and `shared.rs` reference them via `super::FOO`. Promoting
  them all to sub-modules would cascade the cross-module surface
  beyond the R271i threshold.
- `Mempool::max_bytes` field promoted to `pub(super)` so `SharedMempool`
  can read it for its `Default::default()` capacity-mirror behavior.
- `Mempool::insert_checked` / `insert_checked_with_eviction` /
  `revalidate_against_protocol_params` use `ProtocolParameters` from
  `yggdrasil_ledger::*`. inner.rs imports `validate_fee`,
  `validate_tx_ex_units`, `validate_tx_size` directly from
  `yggdrasil_ledger`.

### Visibility / dependency fixups

1. **Import segregation** — runtime-facing imports split between
   inner.rs and shared.rs; queue.rs (residual) keeps only what the
   types it still hosts need (`Era`, `LedgerError`,
   `MultiEraSubmittedTx`, `ShelleyTxIn`, `SlotNo`, `TxId`).
2. **Test block** — `#[cfg(test)] mod tests` block kept in queue.rs
   shell. Tests use `super::*` which resolves through queue.rs's
   `pub use inner::Mempool;` and `pub use shared::SharedMempool;`
   re-exports — no test rewrites needed.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/consensus/src/mempool/queue.rs` | 1,665 | 731 | −934 |
| `crates/consensus/src/mempool/queue/inner.rs` | (new) | 658 | +658 |
| `crates/consensus/src/mempool/queue/shared.rs` | (new) | 324 | +324 |

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
| **R273d (mempool/queue)** | **`queue/{inner,shared}.rs`** | **1,665 → 731 (−934)** |

Total moved: ~1,956 lines across 8 sub-modules.

### Stop point — R273e is the next consensus-crate slice

Remaining ≥600-line consensus files:

| File | Lines | Likely split |
|---|---|---|
| `crates/consensus/src/mempool/tx_state.rs` | 768 | per-state machine |
| `crates/consensus/src/diffusion_pipelining.rs` | 747 | per-state-machine slice |
| `crates/consensus/src/chain_state.rs` | 654 | volatile / immutable / dep-state split |

R273e candidate: split `mempool/tx_state.rs` since it's structurally
similar to queue.rs (cross-peer TxId dedup state machine) and would
keep R273 momentum on the mempool subsystem.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R273
- R273c closure: `2026-05-07-round-273c-opcert-split.md`
- Upstream Mempool API:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Mempool/API.hs`
- Upstream Mempool Impl.Update:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Mempool/Impl/Update.hs`
- Upstream Mempool top-level wrapper:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Mempool.hs`
