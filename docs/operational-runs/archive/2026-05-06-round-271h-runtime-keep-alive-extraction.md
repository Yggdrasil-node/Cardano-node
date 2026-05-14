## Round 271h — `runtime.rs` per-domain split: eighth slice (KeepAliveScheduler + adjacent trace helpers)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R271 eighth slice)

### Slice scope

Extracted 76 source lines from `runtime.rs` into a new
`node/src/runtime/keep_alive.rs` (~100 lines). Items moved:

- `KEEPALIVE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20)` —
  20-second cadence, well below upstream's ~97s `keepAliveTimeout`.
- `struct KeepAliveScheduler` — heartbeat tracker (`last_sent_at`,
  `next_cookie: u16`).
- `impl KeepAliveScheduler` with `new` and `tick` methods.
- `fn trace_sync_failure` — `Node.Sync` Error trace emitter.
- `fn trace_verified_sync_batch_applied` — `ChainSync.Client` Info trace.

The two `trace_*` fns came along because they sat directly after the
KeepAliveScheduler impl in source order — the file-position-based
extraction grabbed them as part of the cluster. They're conceptually
"sync tracing helpers" rather than keep-alive-specific, but bundling
them with KeepAliveScheduler avoids a pointless additional split for
two ~15-line fns.

`runtime.rs` keeps a `mod keep_alive;` declaration plus a `use
keep_alive::{KeepAliveScheduler, trace_sync_failure,
trace_verified_sync_batch_applied};` (the constant
`KEEPALIVE_HEARTBEAT_INTERVAL` is only referenced inside keep_alive.rs
itself, so it stays unexported).

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `runtime/keep_alive.rs::KeepAliveScheduler` + `KEEPALIVE_HEARTBEAT_INTERVAL` | upstream `Ouroboros.Network.Protocol.KeepAlive.Client` heartbeat client (yggdrasil-side wrapper around `KeepAliveClient::keep_alive` with our chosen 20s cadence) |
| `runtime/keep_alive.rs::trace_sync_failure` / `trace_verified_sync_batch_applied` | yggdrasil-only — operational tracing on top of upstream's `traceWith` for `ChainSync.Client` and `Node.Sync` namespaces |

### Visibility / dependency fixups

This slice required four `pub(super) fn` / `pub(super) const` /
`pub(super) struct` promotions because the bulk-extraction landed
items that runtime.rs still calls:

1. `KEEPALIVE_HEARTBEAT_INTERVAL` const: `const` → `pub(super) const`
2. `KeepAliveScheduler` struct: `struct` → `pub(super) struct`
3. `KeepAliveScheduler::new` method: `fn` → `pub(super) fn`
4. `KeepAliveScheduler::tick` method: `async fn` → `pub(super) async fn`
5. `trace_sync_failure` free fn: `fn` → `pub(super) fn`
6. `trace_verified_sync_batch_applied` free fn: `fn` → `pub(super) fn`

The trace fns reference `super::ReconnectingRunState`,
`super::BatchTraceExtras`, `super::sync_error_trace_fields`, and
`super::verified_sync_batch_trace_fields` — all private to runtime.rs
but reachable from the descendant module via the standard
descendants-see-ancestors visibility rule (no parent-side promotions
needed).

Plus runtime.rs `yggdrasil_network` import block trimmed: dropped
`KeepAliveClient`, `KeepAliveClientError` (used only by the moved
KeepAliveScheduler impl, no longer referenced in residual runtime.rs).

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 6,008 | 5,934 | −74 |
| `node/src/runtime/keep_alive.rs` | (new) | ~100 | +100 |

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
| **R271h (KeepAliveScheduler + trace helpers)** | **`runtime/keep_alive.rs` (~100)** | **74** | **5,934** |

Net `runtime.rs` reduction so far: **7,269 → 5,934 lines (−1,335, ~18 %)**.

### Stop point — R271i (the big async fns) is the next slice

Remaining content in runtime.rs (~5,934 lines):
- `ReconnectingRunState` struct + 2 impls (~600 lines)
- The big `run_governor_loop` async fn (~1,000 lines)
- `run_block_producer_loop` async fn (~500 lines)
- `run_reconnecting_verified_sync_service*` family (4 entry points + helpers) (~3,000 lines)
- Various helper free fns

R271i+ will tackle the orchestration fns. Each `async fn` is large
enough to be its own file (per-fn split), or smaller helpers can be
grouped by family (e.g. `runtime/sync_helpers.rs`,
`runtime/governor_loop.rs`, `runtime/block_producer_loop.rs`).

### Lesson (recurring): bulk-extraction grabs adjacent fns

Three R271 rounds (R271d, R271f, R271h) have surfaced the same
pattern: when extracting a "named cluster" by line range, the
boundary commonly grabs 1-2 adjacent fns that aren't strictly part of
the cluster's named target. Two ways to handle:

- **Re-include them in the same module** if they're conceptually
  related (R271h's `trace_*` helpers — they're "sync tracing" but no
  worse located here than in runtime.rs).
- **Split them off** if they belong elsewhere (no cases yet — the
  per-fn cohesion of the runtime.rs source ordering has been close
  enough to upstream module boundaries).

The visibility-promotion ratio (number of `pub(super)` adds per
extraction) is the practical signal: 2-4 promotions = "natural
cluster"; 6+ promotions = "the cluster is leaking parent state and
should be smaller". R271h hit 6 promotions (the high end) but stayed
in scope because the trace fns are simple wrappers.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271g closure: `2026-05-06-round-271g-runtime-bootstrap-extraction.md`
- Upstream KeepAlive client: `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/KeepAlive/Client.hs`
