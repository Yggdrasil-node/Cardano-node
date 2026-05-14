## Round 271i — `runtime.rs` per-domain split: ninth slice ATTEMPT (ReconnectingRunState) — rolled back

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R271 ninth slice attempt — rollback)

### Slice scope (attempted)

Tried to extract the reconnecting verified-sync orchestrator state cluster
(~462 lines, lines 2947–3408 of post-R271h runtime.rs):

- `struct ReconnectingVerifiedSyncContext<'a>` — input bundle.
- `struct ReconnectingVerifiedSyncState` — per-attempt mutable state.
- `struct ReconnectingRunState` — run-level statistics + transient
  per-batch state.
- `struct RollbackReAdmissionStats` — Slice GD bookkeeping for txs
  re-admitted from mempool after rollback.
- `struct BatchTraceExtras` — per-batch trace bundle.
- Two `impl ReconnectingRunState` blocks (~280 lines combined) with
  the `_runstate_impl_marker` module boundary preserver.
- `enum BatchErrorDisposition` + `fn record_verified_batch_progress`
  + `fn session_established_trace_fields` + helper trace fns.

### Why rolled back

The cluster has too many **tightly-coupled internal dependencies** to
parent runtime.rs items that aren't worth promoting to `pub(super)` for
a single extraction:

1. **`MultiEraSyncStep`** — type defined elsewhere in runtime.rs (or
   crate::sync), referenced by the cluster's batch trace helpers.
2. **`trace_fields`** — private helper in runtime.rs body for trace
   field assembly.
3. **`peer_point_trace_fields`** — private helper in runtime.rs body.
4. **`tests.rs` super:: imports** — `runtime/tests.rs` (descendant
   module) imports `BatchErrorDisposition`, `record_verified_batch_progress`,
   `session_established_trace_fields` directly via `super::`. Moving
   them into `runtime/reconnecting.rs` breaks tests.rs's path
   resolution unless tests.rs is updated too.

The combined surface — internal trace helpers + tests.rs imports +
the 8+ types/fns in the cluster — would require ~15+ visibility
promotions plus restructuring tests.rs imports. That's beyond the
"natural cluster" threshold the prior R271 slices have respected.

### Lessons (recurring)

1. **Visibility-promotion count predicts cluster fit.** R271h hit 6
   promotions and stayed clean; R271i would need 15+. Empirical rule:
   if a cluster needs more than ~8 `pub(super)` adds, the extraction is
   leaking parent state and the cluster boundary is wrong.

2. **Test-file `super::*` imports complicate descendant-module
   extraction.** When `runtime/tests.rs` (a descendant of runtime)
   imports symbols via `super::*` rather than fully-qualified paths,
   moving those symbols to a sibling sub-module breaks the test build.
   Future runtime.rs extractions should grep tests.rs first for
   `super::FOO` references and check if the target items appear there.

3. **The right slice may need a "trace helpers first" prelude.** The
   tracing helpers (`trace_fields`, `peer_point_trace_fields`,
   `sync_error_trace_fields`, etc.) are the most cross-cutting items.
   Extracting them into a `runtime/tracing.rs` first would reduce the
   coupling for subsequent reconnecting/loop extractions.

### Rollback procedure

Used Python script to (a) read the current `runtime.rs`, (b) read the
attempted `reconnecting.rs`, (c) replace the `mod reconnecting; use
reconnecting::{...};` block in runtime.rs with the bulk body content
from reconnecting.rs (skipping the imports header), (d) delete
`reconnecting.rs`. Verified with all 4 cargo gates green at the
post-R271h state.

### Diff (after rollback)

| File | Lines before R271i | Lines after rollback | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 5,934 | 5,934 | 0 |
| `node/src/runtime/reconnecting.rs` | (didn't exist) | (deleted) | — |

### Verification gates (post-rollback)

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged from R271h)
```

### Next slice (revised plan)

Better order for the remaining R271 work:

1. **R271i (revised):** Extract `runtime/tracing.rs` with the
   trace-helpers family (`trace_fields`, `peer_point_trace_fields`,
   `sync_error_trace_fields`, `verified_sync_batch_trace_fields`,
   `session_established_trace_fields`, `peer_point_session_established_trace_fields`,
   etc.). ~150-200 lines. Once trace fns live in their own module,
   subsequent extractions can include these as `super::tracing::*`
   imports cleanly.

2. **R271j:** Re-attempt the ReconnectingRunState extraction with the
   trace dependency already in `runtime/tracing.rs`. Should be a much
   smaller surface.

3. **R271k+:** The big async fns (`run_governor_loop`,
   `run_block_producer_loop`, `run_reconnecting_verified_sync_service*`).

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271h closure: `2026-05-06-round-271h-runtime-keep-alive-extraction.md`
- Empirical promotion-count rule documented in R271h: "2-4 promotions
  = natural cluster; 6+ = leaking parent state". R271i would have
  needed 15+, well above threshold.
