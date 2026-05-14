## Round 271i (revised) — `runtime.rs` per-domain split: ninth slice (Trace-field builders)

Date: 2026-05-07
Branch: main
Type: Filename-mirror refactor (Phase γ R271 ninth slice — narrower scope after R271i original rollback)

### Slice scope

Extracted 56 source lines from `runtime.rs` into a new
`node/src/runtime/tracing.rs` (92 lines). Items moved (4 trace-field
builders):

- `pub(super) fn peer_point_trace_fields(peer_addr, current_point)
  -> BTreeMap<String, Value>` — base bundle (peer + currentPoint).
- `pub(super) fn session_established_trace_fields(peer_addr,
  reconnect_count, from_point) -> BTreeMap<String, Value>` —
  session-bring-up bundle.
- `pub(super) fn sync_error_trace_fields(peer_addr, error,
  current_point) -> BTreeMap<String, Value>` — error-side bundle.
- `pub(super) fn verified_sync_batch_trace_fields(peer_addr,
  current_point, progress, run_state, extras) -> BTreeMap<String,
  Value>` — batch-application bundle (slot, blocks, rollbacks, era
  distribution, density).

`runtime.rs` keeps a `mod tracing;` declaration plus a `use
tracing::{…};` import (4 names) so existing call sites continue to
resolve unchanged.

### Why this slice (revised after R271i original rollback)

The original R271i attempted to extract the full `ReconnectingRunState`
cluster (~462 lines) and discovered that:

1. The cluster's `verified_sync_batch_trace_fields` and friends are
   referenced from `runtime/tests.rs` via `super::*`.
2. The cluster's `record_verified_batch_progress` and other helpers
   have private parent-state dependencies that need ~15+ `pub(super)`
   promotions.

R271i revised takes the trace-field builders alone — they're the
shared dependency that a clean `ReconnectingRunState` extraction needs
to have already moved. Once the builders live in their own
`runtime/tracing.rs`, R271j (the next attempt at ReconnectingRunState)
can `use super::tracing::{...}` instead of relying on cluster-internal
visibility, dropping the promotion count well below the threshold.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `runtime/tracing.rs::peer_point_trace_fields` etc. | upstream `Cardano.Node.Tracing.Tracers.NodeToNode.*` data-side trace bundles emitted via `traceWith` to `Tracer (BTreeMap String Aeson.Value)` (yggdrasil's runtime tracer hooks the same field bundles into `NodeTracer::trace_runtime`) |

### Visibility note

All 4 fns promoted to `pub(super)` so the parent runtime.rs and its
descendant `runtime/tests.rs` can both call them. No cross-cutting
parent imports needed in tracing.rs — it imports `super::BatchTraceExtras`
+ `super::ReconnectingRunState` (via the standard descendants-see-
ancestors rule for those two parent-private types).

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 5,934 | 5,879 | −55 |
| `node/src/runtime/tracing.rs` | (new) | 92 | +92 |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

**Clean first-try extraction.** The narrower slice scope avoided all
the cascade issues of the original R271i attempt.

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
| R271i (original) | (rolled back) | 0 | 5,934 |
| **R271i (revised — Trace-field builders)** | **`runtime/tracing.rs` (92)** | **55** | **5,879** |

Net `runtime.rs` reduction so far: **7,269 → 5,879 lines (−1,390, ~19 %)**.

### Stop point — R271j re-attempts ReconnectingRunState with tracing.rs as prelude

With trace-field builders now in `runtime/tracing.rs`, R271j can
re-attempt the ReconnectingRunState cluster extraction. The remaining
super-side dependencies should be:

- `super::tracing::*` — already extracted ✓
- `super::peer_session::NodeConfig` — already extracted (R271f) ✓
- `super::block_producer_config::SharedBlockProducerState` — already
  extracted (R271b) ✓
- `super::ChainTipNotify` + `super::CheckpointTracking` — still in
  runtime.rs but trivially re-exportable.

Estimated promotion count for R271j: 3-5 (vs 15+ for the original
attempt) — well under the "natural cluster" threshold.

### Lesson confirmed

The "extract shared deps first" pattern works. When a target cluster
shares 5+ parent-private symbols with a different test/sibling
module, do the prelude extraction first to break the chain.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271h closure: `2026-05-06-round-271h-runtime-keep-alive-extraction.md`
- R271i original rollback: `2026-05-06-round-271i-runtime-reconnecting-extraction-rollback.md`
- Upstream tracer field bundles: `.reference-haskell-cardano-node/cardano-node/src/Cardano/Node/Tracing/Tracers/NodeToNode.hs`
