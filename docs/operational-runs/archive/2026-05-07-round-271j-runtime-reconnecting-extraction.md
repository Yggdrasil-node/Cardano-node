## Round 271j — `runtime.rs` per-domain split: tenth slice (ReconnectingRunState cluster)

Date: 2026-05-07
Branch: main
Type: Filename-mirror refactor (Phase γ R271 tenth slice — re-attempt of R271i original after the trace-helpers prelude landed)

### Slice scope

Extracted 443 source lines from `runtime.rs` into a new
`node/src/runtime/reconnecting.rs` (503 lines including module
docstring + imports). This is the cluster R271i (original) tried and
rolled back; R271j succeeds because R271i (revised) extracted the
trace-field builders into `runtime/tracing.rs` first.

Items moved (5 structs, 1 enum, 9 free fns, 2 impl blocks):

**Orchestrator state:**
- `ReconnectingVerifiedSyncContext<'a>` — 14-field input bundle.
- `ReconnectingVerifiedSyncState` — per-attempt mutable state.
- `ReconnectingRunState` — run-level statistics (block totals,
  rollback counts, batches completed, reconnect counts, consecutive
  failures, etc.) plus 2 impl blocks separated by the
  `_runstate_impl_marker` module.
- `RollbackReAdmissionStats` — 7-field bookkeeping for rollback
  re-admission tallies.
- `BatchTraceExtras` — per-batch trace bundle.
- `BatchErrorDisposition` enum — three-way error disposition.

**Free fns:**
- `cache_confirmed_entries`, `re_admit_rolled_back_tx_ids`,
  `evict_mempool_after_roll_forward` — mempool eviction/re-admission.
- `pool_register_peer`, `pool_unregister_peer`,
  `pool_update_fragment_head`, `pool_should_demote_peer` — peer pool
  state helpers.
- `registry_mark_bootstrap_hot`, `registry_mark_bootstrap_cooling` —
  registry transition helpers.
- `record_verified_batch_progress` — batch accumulator updater.

`runtime.rs` keeps a `mod reconnecting;` declaration plus a `use
reconnecting::{…};` import (lib build) plus a `#[cfg(test)] use
reconnecting::cache_confirmed_entries;` (test build only — only
tests.rs references this helper).

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `runtime/reconnecting.rs::ReconnectingVerifiedSyncContext` + `::ReconnectingVerifiedSyncState` + `::ReconnectingRunState` | upstream `Ouroboros.Consensus.Node.Run.runWith` reconnect loop state record |
| `runtime/reconnecting.rs::record_verified_batch_progress` | upstream batch-application accumulator updates inside `peerBlockFetchClient` |
| `runtime/reconnecting.rs::evict_mempool_after_roll_forward` | upstream `Ouroboros.Consensus.Mempool.Update.syncWithLedger` mempool revalidation after roll-forward |
| `runtime/reconnecting.rs::re_admit_rolled_back_tx_ids` | yggdrasil-only — Slice GD bookkeeping for txs rolled back from confirmation |

### Visibility model — 30+ promotions

This slice required the largest visibility-promotion count of any R271
round. Tracked separately:

1. **Item-level promotions** (`fn` → `pub(super) fn`, `struct` → `pub(super) struct`, `enum` → `pub(super) enum`):
   - 5 structs × 1 = 5
   - 1 enum × 1 = 1
   - 9 free fns × 1 = 9
   - ~12 impl methods × 1 = 12
   - **Subtotal: ~27 item promotions.**

2. **Field-level promotions** (`field:` → `pub(super) field:`):
   - `ReconnectingVerifiedSyncContext` ~14 fields × 1 = 14
   - `ReconnectingVerifiedSyncState` 3 fields × 1 = 3
   - `ReconnectingRunState` 7 fields × 1 = 7
   - `RollbackReAdmissionStats` 7 fields × 1 = 7
   - `BatchTraceExtras` 2 fields × 1 = 2
   - **Subtotal: ~33 field promotions.**

Total ~60 `pub(super)` promotions.

The "natural cluster threshold" of 6 promotions is meant for the
**item count, not field count.** Counting items only, R271j hit ~27,
still well past the 6-threshold — but it was tractable because:

- Items group cleanly: 5 struct types + 1 enum + 9 free fns + impl methods.
- All references from runtime.rs and tests.rs flow through the same
  re-export at the parent.
- The trace-field builders had already been extracted (R271i revised),
  removing the deepest cross-cutting dependency.

The threshold is empirical, not absolute. Past 27 item promotions but
with a clear semantic boundary, the extraction works. Past 27 with
distributed/leaking dependencies (R271i original), it doesn't.

### Edit-tooling note: dual scripts for promotion + parameter cleanup

Used a 2-stage Python script:

1. **Bulk promotion script:** added `pub(super)` to all matching
   patterns: `^struct X`, `^enum X`, `^fn X(`, `^    fn X(`, `^    async fn X(`, and `^    field:`.
2. **Parameter cleanup script:** the bulk script over-promoted (added
   `pub(super)` to fn parameter lines that look like fields with the
   same `^    name: Type,` pattern). Second script tracked paren depth
   to identify fn signatures and reverted `pub(super)` inside parens.

This pair is reusable for future per-domain extractions; the parameter
cleanup is the new lesson on top of R270's plain `pub(super)` work.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 5,879 | 5,444 | −435 |
| `node/src/runtime/reconnecting.rs` | (new) | 503 | +503 |

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
| R271i original | (rolled back) | 0 | 5,934 |
| R271i revised (Trace-field builders) | `runtime/tracing.rs` (92) | 55 | 5,879 |
| **R271j (ReconnectingRunState cluster)** | **`runtime/reconnecting.rs` (503)** | **435** | **5,444** |

Net `runtime.rs` reduction so far: **7,269 → 5,444 lines (−1,825, ~25 %)**.

### Stop point — R271k+ the big async fns

Remaining content in runtime.rs (~5,444 lines):
- The big `run_governor_loop` async fn (~1,000 lines)
- `run_block_producer_loop` async fn (~500 lines)
- `run_reconnecting_verified_sync_service*` family (4 entry points + helpers) (~3,000 lines)
- Various smaller helper fns

R271k+ tackles the orchestration fns. With the orchestrator state
already in `runtime/reconnecting.rs`, those async fns should extract
cleanly as `super::reconnecting::*` consumers.

### Lesson confirmed

The "extract shared deps first" pattern is the right strategy when a
target cluster has cross-cutting parent-private symbols. R271i original
hit those symbols head-on and rolled back. R271i revised extracted
just the shared-dep prelude (trace-field builders, 56 lines). R271j
then completed the full ReconnectingRunState extraction with ~60
promotions but no cross-cutting blockers.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271i revised closure: `2026-05-07-round-271i-revised-runtime-tracing-extraction.md`
- R271i original rollback: `2026-05-06-round-271i-runtime-reconnecting-extraction-rollback.md`
- Upstream reconnect loop: `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Node/Run.hs`
