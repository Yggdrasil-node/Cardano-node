## Round 271g — `runtime.rs` per-domain split: seventh slice (Bootstrap entry points)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R271 seventh slice)

### Slice scope

Extracted 156 source lines from `runtime.rs` into a new
`node/src/runtime/bootstrap.rs` (188 lines). Items moved (3 async fns):

- `bootstrap(config) -> Result<PeerSession, PeerError>` — single-peer
  convenience wrapper.
- `bootstrap_with_fallbacks(config, fallback_addrs) -> Result<PeerSession, PeerError>` —
  primary plus ordered fallbacks.
- `bootstrap_with_attempt_state(config, &mut PeerAttemptState, &NodeTracer) -> Result<PeerSession, PeerError>` —
  underlying driver shared by the above; promoted from `async fn`
  (private) to `pub async fn` (visible) so it can be re-exported and
  called from the residual `ReconnectingRunState` orchestration code
  in runtime.rs (3 call sites at lines 4188, 4853, 5448).

`runtime.rs` keeps a `pub mod bootstrap;` declaration plus a
`pub use bootstrap::{bootstrap, bootstrap_with_attempt_state,
bootstrap_with_fallbacks};` re-export.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `runtime/bootstrap.rs::bootstrap*` | upstream `Ouroboros.Network.NodeToNode.connectTo` (TCP connect + mux handshake + version negotiation) plus `Ouroboros.Network.Subscription.Worker` fallback iteration |
| `runtime/bootstrap.rs::bootstrap_with_attempt_state` | upstream subscription worker's per-attempt loop body — yggdrasil collapses it into a simple ordered retry with duplicate-skip via `PeerAttemptState` |

### Visibility / dependency fixups

This slice required several iterative fixups due to the wide
dependency surface of NtN connection bring-up:

1. **`serde_json::json` macro** — used inside `trace_fields(...)`
   calls for structured tracing. Added `use serde_json::json;`.
2. **`trace_fields` helper** — added `use crate::tracer::trace_fields;`
   alongside the existing `NodeTracer` import.
3. **`yggdrasil_network` types pulled in** — bootstrap walks all 5
   mini-protocol clients during session construction:
   `BlockFetchClient`, `ChainSyncClient`, `HandshakeVersion`,
   `KeepAliveClient`, `MiniProtocolNum`, `NodeToNodeVersionData`,
   `PeerAttemptState`, `PeerConnection`, `PeerError`,
   `PeerSharingClient`, `TxSubmissionClient`, plus `peer_attempt_state`
   constructor fn.
4. **`bootstrap_with_attempt_state` promoted** from `async fn` →
   `pub async fn` so the re-export at runtime.rs makes it reachable
   from the remaining `ReconnectingRunState` callers.
5. **runtime.rs imports trimmed** — dropped now-unused
   `HandshakeVersion`, `PeerConnection`, `PeerError`,
   `PeerSharingClient`, `TxSubmissionClient` (the 5 types used
   exclusively by the moved bootstrap fns). The remaining residual
   runtime.rs code still uses several yggdrasil_network types so the
   import block stayed substantial.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 6,166 | 6,008 | −158 |
| `node/src/runtime/bootstrap.rs` | (new) | 188 | +188 |

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
| **R271g (Bootstrap entry points)** | **`runtime/bootstrap.rs` (188)** | **158** | **6,008** |

Net `runtime.rs` reduction so far: **7,269 → 6,008 lines (−1,261, ~17 %)**.

### Stop point — R271h (the remaining sync-session helpers + governor/block-producer loops) is the next slice

Remaining content in runtime.rs (~6,000 lines):
- `ReconnectingRunState` struct + 2 impls (~600 lines)
- `KeepAliveScheduler` struct + impl (~200 lines)
- The big `run_governor_loop` async fn (~1,000 lines)
- The big `run_block_producer_loop` async fn (~500 lines)
- `run_reconnecting_verified_sync_service*` family (4 entry points + helpers) (~3,000 lines)
- Various helper free fns

R271h+ will tackle these orchestration fns. Each `async fn` body is
large enough to be its own file (per-fn split per the original blueprint).

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271f closure: `2026-05-06-round-271f-runtime-peer-session-extraction.md`
- Upstream NodeToNode connectTo: `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/lib/Ouroboros/Network/NodeToNode.hs`
- Upstream subscription worker: `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/lib/Ouroboros/Network/Subscription/Worker.hs`
