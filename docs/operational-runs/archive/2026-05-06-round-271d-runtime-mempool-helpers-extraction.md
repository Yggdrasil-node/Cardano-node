## Round 271d — `runtime.rs` per-domain split: fourth slice (Mempool TxSubmission helpers)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R271 fourth slice)

### Slice scope

Extracted 213 source lines from `runtime.rs` into a new
`node/src/runtime/mempool_helpers.rs` (240 lines). Items moved:

**Result + error types:**
- `MempoolAddTxResult` enum (`MempoolTxAdded` / `MempoolTxRejected`)
- `MempoolAddTxError` enum (`Mempool(MempoolError)`)
- `MempoolAddTxOutcome` struct (admission result + evicted TxIds)

**Direct-API helpers:**
- `add_tx_to_mempool(&mut LedgerState, &mut Mempool, ...)`
- `add_txs_to_mempool<I: IntoIterator>(...)`

**Shared-handle API helpers (production path):**
- `add_tx_to_shared_mempool(&mut LedgerState, &SharedMempool, ...)`
- `add_txs_to_shared_mempool<I: IntoIterator>(...)`

**Eviction-aware variants:**
- `add_tx_to_shared_mempool_with_eviction(...)`
- `add_txs_to_shared_mempool_with_eviction<I: IntoIterator>(...)`

**Internal helpers:**
- `admitted_entry(MultiEraSubmittedTx) -> MempoolEntry`
- `add_tx_with<F>(...)` — generic insertion delegate

`runtime.rs` keeps a `pub mod mempool_helpers;` declaration plus a
`pub use mempool_helpers::{…};` re-export listing all 9 public items.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `runtime/mempool_helpers.rs::add_tx_to_*` family | upstream `Ouroboros.Network.TxSubmission.Inbound.Server` calling into `Ouroboros.Consensus.Mempool.Update.addTxs` |
| `runtime/mempool_helpers.rs::*_with_eviction` variants | upstream `Ouroboros.Consensus.Mempool.Impl.Update.makeRoomForTransaction` (lowest-fee eviction to fit a new tx) |
| `runtime/mempool_helpers.rs::MempoolAddTxResult` / `MempoolAddTxError` | upstream `MempoolAddTxResult` + queue-level error split |
| `runtime/mempool_helpers.rs::MempoolAddTxOutcome` | yggdrasil-only — bundles admission result with evicted TxIds for trace observability |

### Visibility / dependency fixups

1. **`MempoolError`, `MempoolEntry`, `SlotNo`, `LedgerError`,
   `MultiEraSubmittedTx`, `PlutusEvaluator`** — all moved into the new
   module's import block from `runtime.rs`'s top-level imports.
2. **`SharedTxState`** — initially imported but unused (the `*_shared_*`
   variants take a `&SharedMempool` directly, not via `SharedTxState`).
   Dropped.
3. **runtime.rs imports trimmed** — `LedgerError`,
   `MultiEraSubmittedTx`, and `plutus_validation::PlutusEvaluator` no
   longer used in runtime.rs's residual code; removed from the
   `yggdrasil_ledger::*` import block.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 6,995 | 6,777 | −218 |
| `node/src/runtime/mempool_helpers.rs` | (new) | 240 | +240 |

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
| **R271d (Mempool helpers)** | **`runtime/mempool_helpers.rs` (240)** | **218** | **6,777** |

Net `runtime.rs` reduction so far: **7,269 → 6,777 lines (−492, ~7 %)**.

### Stop point — R271e (TxSubmissionService types) is the next slice

Remaining major candidates per `docs/REFACTOR_BLUEPRINT.md`:

| Round | Target | Approx lines |
|---|---|---|
| R271e | `TxSubmissionService*` types (line ~3144+) | ~150 |
| R271f | `NodeConfig` + `PeerSession` + sync-request types | ~600 |
| R271g | Big sync-session helpers in second half of runtime.rs | ~2,500 |
| R271h+ | sync.rs split (separate arc, 9,567 lines) | many slices |

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271c closure: `2026-05-06-round-271c-runtime-ledger-judgement-extraction.md`
- Upstream addTxs: `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Mempool/Update.hs`
- Upstream makeRoomForTransaction: `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Mempool/Impl/Update.hs`
