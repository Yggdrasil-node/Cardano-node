## Round 270c — `governor.rs` per-domain split: third slice (PeerMetric)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R270 third slice)

### Slice scope

Extracted 440 source lines from `governor.rs` into a new
`crates/network/src/governor/peer_metric.rs` (466 lines including
module docstring + imports). Items moved:

- `PeerFailureRecord` — per-peer failure record with timestamps for
  exponential backoff.
- `RequestBackoffState` — request-style discovery operation backoff
  state.
- `Xorshift64` — deterministic 64-bit xorshift PRNG used by peer-pick
  policy.
- `PickPolicy` — randomized peer-selection policy (matches upstream's
  `LedgerPeers.Utils` pick policy).
- `PeerMetrics` — header / block-fetch latency tracking + density
  scoring (`combined_score`, `set_density`, `LOW_DENSITY_THRESHOLD`,
  `HIGH_DENSITY_BONUS`).
- `HotPeerScheduling` — per-protocol egress weights for hot peers.
- `hot_peers_remote(registry)` — helper returning the set of currently
  hot non-local-root peers.

`governor.rs` keeps a `pub mod peer_metric;` declaration plus a
`pub use peer_metric::{…};` re-export block. Includes the public
constants `LOW_DENSITY_THRESHOLD` and `HIGH_DENSITY_BONUS` so test
modules that reference them as plain identifiers continue to resolve
through the standard `use crate::governor::*` import.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `governor/peer_metric.rs::PeerMetrics`, `HotPeerScheduling` | upstream `Ouroboros.Network.PeerSelection.PeerMetric` (`PeerMetric`, `HeaderMetricsTracer`, `BlockFetchMetricsTracer`) |
| `governor/peer_metric.rs::PickPolicy`, `Xorshift64` | upstream `Ouroboros.Network.PeerSelection.LedgerPeers.Utils` randomized pick logic |
| `governor/peer_metric.rs::PeerFailureRecord`, `RequestBackoffState` | upstream `Ouroboros.Network.PeerSelection.Governor.RootPeers` failure-record bookkeeping |
| `governor/peer_metric.rs::LOW_DENSITY_THRESHOLD`, `HIGH_DENSITY_BONUS` | upstream Genesis-density consensus-side `DEFAULT_LOW_DENSITY_THRESHOLD = 0.6` and density bonus weighting |
| `governor/peer_metric.rs::hot_peers_remote` | yggdrasil-only filter helper used by Slice GD-Governor density calculations |

### Visibility / dependency notes

1. **`is_big_ledger` cross-module reference.** `hot_peers_remote` calls
   the `is_big_ledger(entry)` free fn that lives in `governor.rs`.
   Adjusted the call site to `super::is_big_ledger(entry)` in
   peer_metric.rs since the function is private to governor.rs and the
   submodule needs an explicit super:: path.

2. **`MiniProtocolNum` test-only dependency.** Removing
   `MiniProtocolNum` from `governor.rs`'s imports broke
   `governor/tests.rs` (descendant module via `#[cfg(test)] mod tests;`)
   which uses `MiniProtocolNum` in egress-weight test fixtures. Fixed
   by adding `#[cfg(test)] use crate::multiplexer::MiniProtocolNum;`
   to governor.rs. Same pattern as R270a's `UseLedgerPeers` and R270b's
   `LedgerStateJudgement`.

3. **`PeerRegistryEntry` import slimming.** After the
   peer_metric.rs `hot_peers_remote` was retargeted via `super::`, the
   `PeerRegistryEntry` import in peer_metric.rs became unused (the
   call's argument is `&PeerRegistry`, the entry is unwrapped via the
   iterator). Dropped `PeerRegistryEntry` from peer_metric.rs's
   peer_registry import.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/network/src/governor.rs` | 2,806 | 2,371 | −435 |
| `crates/network/src/governor/peer_metric.rs` | (new) | 466 | +466 |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

### Cumulative R270 progress

| Slice | File created | Lines moved | governor.rs running size |
|---|---|---|---|
| R270a (Types) | `governor/types.rs` (335) | 342 | 3,146 |
| R270b (Churn) | `governor/churn.rs` (360) | 340 | 2,806 |
| **R270c (PeerMetric)** | **`governor/peer_metric.rs` (466)** | **435** | **2,371** |

Net `governor.rs` reduction so far: **3,488 → 2,371 lines (−1,117, ~32 %)**.

### Stop point — R270d (Governor bulk) is the next big slice

Remaining domains in `governor.rs`:

| Round | Mirror target | Approx lines |
|---|---|---|
| R270d | `Ouroboros.Network.PeerSelection.Governor` (the bulk: `GovernorState`, `PeerLifetimeStats`, `GovernorAction`, all `evaluate_*` functions, `governor_tick`, plus private helpers like `is_big_ledger`) | ~1,800 |
| R270e | `Ouroboros.Network.PeerSelection.{Counters, ConnectionManager}` (`PeerSelectionCounters`, `OutboundConnectionsState`, `ConnectionManagerCounters`, `PeerSelectionTimeouts`) | ~600 |

R270d is the ~1,800-line bulk slice that holds the actual governor
decision logic (the evaluate_* family). It's the most complex remaining
extraction in this arc but is structurally the last large piece to
peel off — after R270d, what remains in governor.rs is just the
counters + connection-manager glue (R270e) plus the orchestration
shell.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R270
- R270b closure: `2026-05-06-round-270b-governor-churn-extraction.md`
- Upstream peer-metric: `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/lib/Ouroboros/Network/PeerSelection/PeerMetric.hs`
- Upstream peer-pick policy: `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/lib/Ouroboros/Network/PeerSelection/LedgerPeers/Utils.hs`
- Genesis density consensus-side reference: `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Genesis/Governor.hs`
