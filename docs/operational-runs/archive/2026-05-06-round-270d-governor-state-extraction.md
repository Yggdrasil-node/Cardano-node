## Round 270d — `governor.rs` per-domain split: fourth slice (Governor state + decision functions)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R270 fourth slice — the bulk peel-off)

### Slice scope

Extracted 1,745 source lines from `governor.rs` into a new
`crates/network/src/governor/state.rs` (1,778 lines). This is the
largest single peel-off in the R270 arc and contains the actual
governor decision logic. Items moved:

**Mutable state:**
- `GovernorState` struct + `Default` impl + `impl GovernorState`
- `PeerLifetimeStats` companion struct

**Action emitter:**
- `GovernorAction` enum

**Decision evaluator family (~20 functions):**
- Promotion: `evaluate_cold_to_warm_promotions`,
  `evaluate_warm_to_hot_promotions`, `evaluate_hot_promotions`,
  `evaluate_cold_to_warm_big_ledger_promotions`,
  `evaluate_warm_to_hot_big_ledger_promotions`
- Demotion: `evaluate_hot_to_warm_demotions`,
  `evaluate_warm_to_cold_demotions`,
  `evaluate_hot_to_warm_big_ledger_demotions`,
  `evaluate_warm_to_cold_big_ledger_demotions`
- Discovery: `evaluate_known_peer_discovery`,
  `evaluate_peer_share_requests`, `evaluate_request_public_roots`,
  `evaluate_request_big_ledger_peers`
- Lifecycle: `evaluate_forget_cold_peers`,
  `evaluate_forget_failed_peers`, `enforce_local_root_valency`
- Sensitive mode: `has_only_trustable_established_peers`,
  `evaluate_sensitive_hot_demotions`,
  `evaluate_sensitive_warm_demotions`, `filter_sensitive_promotions`

**Orchestrator:**
- `governor_tick(...)` — the master decision-cycle entry point

**Private helpers (promoted to `pub fn` in state.rs):**
- `is_big_ledger`
- `trustable_local_root_set`
- `is_trustable_peer`

`governor.rs` keeps a `pub mod state;` declaration plus a `pub use
state::{…};` re-export block listing all 23 of the moved items so
existing callers (and `governor/peer_metric.rs`'s `super::is_big_ledger`)
continue to resolve unchanged.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `governor/state.rs::GovernorState`, `PeerLifetimeStats` | upstream `Ouroboros.Network.PeerSelection.Governor.PeerSelectionState::PeerSelectionState` |
| `governor/state.rs::GovernorAction` | upstream `Ouroboros.Network.PeerSelection.Governor.Types::Decision` action emitter |
| `governor/state.rs::evaluate_*` family | upstream `Ouroboros.Network.PeerSelection.Governor.Monitor` decision rules (one yggdrasil fn per upstream STM transaction) |
| `governor/state.rs::governor_tick` | upstream `peerSelectionGovernor` outer loop body |
| `governor/state.rs::has_only_trustable_established_peers`, `is_trustable_peer`, `trustable_local_root_set` | upstream `LocalRootPeers`/`PeerTrustable` reachability predicates inside `outboundConnectionsState` |

### Visibility / dependency fixups

1. **`is_big_ledger` cross-submodule reference.** Promoted from `fn` to
   `pub fn` in state.rs so its sibling `peer_metric.rs::hot_peers_remote`
   can keep using `super::is_big_ledger` (resolves through the
   `pub use state::is_big_ledger;` re-export at governor.rs).

2. **Counters / connection-manager block (still in governor.rs).** The
   residual `compute_outbound_connections_state` and friends call
   `trustable_local_root_set` / `is_trustable_peer`. Promoted both to
   `pub fn` in state.rs so the residual code resolves them through the
   re-export. (R270e will move counters out next.)

3. **Top-level imports trimmed + `#[cfg(test)]`-gated.** After moving
   the bulk, governor.rs's residual imports were:
   - Lib build: `Duration`, `PeerRegistry`, `PeerSource`, `PeerStatus`,
     `UseBootstrapPeers` (used by counters block).
   - Test build only: `LedgerStateJudgement`, `MiniProtocolNum`,
     `UseLedgerPeers`, `SocketAddr`, `Instant` — all gated with
     `#[cfg(test)]` because `governor/tests.rs` (descendant) uses them
     while the lib build doesn't.

   This is the now-established pattern for governor sub-module
   extractions (4th occurrence of `#[cfg(test)] use ...` for descendant
   test code, after R270a–c).

### Edit-tooling note: large-block extraction via Python with regex anchors

This round's 1,745-line extraction used the same Python pattern as the
state.rs/eras/conway.rs extraction (R269w):

1. Locate the bulk start and end via regex (`pub struct GovernorState`
   doc start, `governor_tick` body close).
2. Walk back from the doc start to include the section divider.
3. Replace the bulk's `fn is_big_ledger(...)` with `pub fn
   is_big_ledger(...)` so descendants can resolve via re-export.
4. Delete the bulk in one slice; insert the re-export block.

Took two iterations to settle the import set:
- First pass: missed the `trustable_local_root_set` / `is_trustable_peer`
  cross-module references from the residual counters block.
- Second pass: missed that the residual code uses different
  std/std::time imports than the bulk; restored what's needed for the
  remaining counters/connection-manager code, gated test-only imports
  with `#[cfg(test)]`.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/network/src/governor.rs` | 2,371 | 644 | −1,727 |
| `crates/network/src/governor/state.rs` | (new) | 1,778 | +1,778 |

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
| R270c (PeerMetric) | `governor/peer_metric.rs` (466) | 435 | 2,371 |
| **R270d (Governor state + evaluators)** | **`governor/state.rs` (1,778)** | **1,727** | **644** |

Net `governor.rs` reduction so far: **3,488 → 644 lines (−2,844, ~82 %)**
across 4 sub-modules.

### Stop point — R270e is the final slice

Remaining content in governor.rs (~644 lines):
- `PeerSelectionCounters` struct + impl (`peerSelectionStateToView` mirror)
- `OutboundConnectionsState` enum + `compute_outbound_connections_state`
- `PeerSelectionTimeouts` struct + Default
- `ConnectionManagerCounters` struct + impl + `Add`

R270e will split these into `governor/{counters,connection_manager,timeouts}.rs`
or bundle them into a single `governor/connection.rs` (depending on
upstream module mapping). After R270e, the per-domain governor split is
complete and Phase γ moves on to R271 (`node/src/{runtime,sync}.rs`
split per `docs/REFACTOR_BLUEPRINT.md`).

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R270
- R270c closure: `2026-05-06-round-270c-governor-peer-metric-extraction.md`
- Upstream peerSelectionGovernor: `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/lib/Ouroboros/Network/PeerSelection/Governor.hs`
- Upstream Monitor decisions: `.reference-haskell-cardano-node/deps/ouroboros-network/cardano-diffusion/lib/Cardano/Network/PeerSelection/Governor/Monitor.hs`
- Upstream PeerSelectionState: `.reference-haskell-cardano-node/deps/ouroboros-network/cardano-diffusion/lib/Cardano/Network/PeerSelection/Governor/PeerSelectionState.hs`
