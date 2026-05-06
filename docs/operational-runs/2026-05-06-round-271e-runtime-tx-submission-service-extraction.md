## Round 271e — `runtime.rs` per-domain split: fifth slice (TxSubmission2 service helpers)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R271 fifth slice)

### Slice scope

Extracted 242 source lines from `runtime.rs` into a new
`node/src/runtime/tx_submission_service.rs` (273 lines). Items moved:

**Result + outcome types:**
- `TxSubmissionServiceError` enum (wraps `TxSubmissionClientError`)
- `TxSubmissionServiceOutcome` struct (handled requests + protocol-vs-shutdown discriminator)

**Internal trait + impls (private to the module):**
- `trait TxSubmissionSnapshotReader { fn mempool_get_snapshot(&self) -> MempoolSnapshot; }`
- `impl TxSubmissionSnapshotReader for TxSubmissionMempoolReader<'_>`
- `impl TxSubmissionSnapshotReader for SharedTxSubmissionMempoolReader`

**Per-request server helpers:**
- `serve_txsubmission_request_from_snapshot_reader<R>` (private generic over `R: TxSubmissionSnapshotReader`)
- `serve_txsubmission_request_from_reader` (pub thin wrapper for direct readers)
- `serve_txsubmission_request_from_mempool` (pub direct-mempool variant)

**Run-loop entry points:**
- `run_txsubmission_service<F>` (direct, takes `&mut Mempool`)
- `run_txsubmission_service_shared<F>` (production, takes `&SharedMempool`)

`runtime.rs` keeps a `pub mod tx_submission_service;` declaration plus
a `pub use tx_submission_service::{…};` re-export for the 6 public
items.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `runtime/tx_submission_service.rs::run_txsubmission_service*` | upstream `Ouroboros.Network.TxSubmission.Inbound.Server` `txSubmissionInbound` driver loop |
| `runtime/tx_submission_service.rs::TxSubmissionSnapshotReader` trait + 2 impls | yggdrasil-only — abstraction over direct + shared reader paths so the per-request serve logic is identical regardless of mempool ownership |
| `runtime/tx_submission_service.rs::serve_txsubmission_request_*` | upstream per-message handlers in `Ouroboros.Network.TxSubmission.Inbound.Server.serverIdle` |

### Visibility / dependency fixups

1. **`TxId`** — referenced in the cluster's `*_from_mempool` body via
   `HashSet<TxId>`/`HashMap<TxId, ...>`. Initially missed; added
   `use yggdrasil_ledger::TxId;` to the new module.
2. **`std::future::Future`** — referenced via the `where F: Future<Output = ()>`
   clauses. Added `use std::future::Future;`.
3. **`MempoolError`** — initially imported but unused in the new
   module (the cluster doesn't reference `MempoolError` directly,
   only through `TxSubmissionClientError`'s wrapping). Dropped.
4. **runtime.rs imports trimmed** — dropped `MEMPOOL_ZERO_IDX`,
   `Mempool`, `MempoolIdx`, `MempoolSnapshot`,
   `SharedTxSubmissionMempoolReader`, `TxSubmissionMempoolReader`,
   `TxIdAndSize`, `TxServerRequest`, `TxSubmissionClientError`. The
   residual runtime.rs code uses neither. Kept `MEMPOOL_ZERO_IDX`,
   `MempoolError` (used by other residual code), `MempoolEntry`,
   `SharedMempool`, `SharedTxState`.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 6,777 | 6,543 | −234 |
| `node/src/runtime/tx_submission_service.rs` | (new) | 273 | +273 |

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
| **R271e (TxSubmission service)** | **`runtime/tx_submission_service.rs` (273)** | **234** | **6,543** |

Net `runtime.rs` reduction so far: **7,269 → 6,543 lines (−726, ~10 %)**.

### Stop point — R271f (NodeConfig + PeerSession + sync-request types) is the next slice

Remaining major candidates per `docs/REFACTOR_BLUEPRINT.md`:

| Round | Target | Approx lines |
|---|---|---|
| R271f | `NodeConfig` + `PeerSession` + `*VerifiedSyncRequest` types | ~600 |
| R271g | Big sync-session helpers in second half of runtime.rs | ~2,500 |
| R271h+ | sync.rs split (separate arc, 9,567 lines) | many slices |

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271d closure: `2026-05-06-round-271d-runtime-mempool-helpers-extraction.md`
- Upstream TxSubmission inbound server: `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/lib/Ouroboros/Network/TxSubmission/Inbound/Server.hs`
- Upstream TxSubmission2 protocol codec: `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/TxSubmission2/Codec.hs`
