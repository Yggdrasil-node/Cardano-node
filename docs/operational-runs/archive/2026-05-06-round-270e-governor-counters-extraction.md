## Round 270e — `governor.rs` per-domain split: fifth and final slice (Counters / outbound-connections / timeouts / ConnMgr counters) — R270 arc complete

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R270 final slice — R270 arc closure)

### Slice scope

Extracted 577 source lines from `governor.rs` into a new
`crates/network/src/governor/counters.rs` (604 lines including module
docstring + imports). Items moved (the entire view-and-orchestration
layer):

- `PeerSelectionCounters` struct (24 fields across regular / big-ledger /
  local-root / non-root categories) + impl `from_registry`,
  `Default`, etc.
- `OutboundConnectionsState` enum + `compute_outbound_connections_state`
  decision function.
- `PeerSelectionTimeouts` struct (configurable policy time constants)
  + `Default` impl.
- `ConnectionManagerCounters` struct + impl + `std::ops::Add` impl.

`governor.rs` keeps a `pub mod counters;` declaration plus a `pub use
counters::{…};` re-export block.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `governor/counters.rs::PeerSelectionCounters` + `from_registry` | upstream `PeerSelectionView Int` (`PeerSelectionCounters` pattern synonym) + `peerSelectionStateToView` in `Ouroboros.Network.PeerSelection.Governor.Types` |
| `governor/counters.rs::OutboundConnectionsState` + `compute_outbound_connections_state` | upstream `OutboundConnectionsState` + `outboundConnectionsState` decision function in `Ouroboros.Network.PeerSelection.Governor.Types` |
| `governor/counters.rs::PeerSelectionTimeouts` | upstream non-pick-function fields of `simplePeerSelectionPolicy` in `Ouroboros.Network.Diffusion.Policies` |
| `governor/counters.rs::ConnectionManagerCounters` + `Add` | upstream `ConnectionManagerCounters` + `Semigroup` instance in `Ouroboros.Network.ConnectionManager.Types` |

### `governor.rs` is now a thin orchestration shell (76 lines)

Final composition of `governor.rs`:

- 12 lines of module docstring (`Reference: Ouroboros.Network.PeerSelection.Governor`)
- 4 lines of explanatory comment about R270 split status
- ~10 lines of `#[cfg(test)] use ...` for descendant `tests.rs` module
  (entirely test-scoped after R270e — the lib build no longer uses any
  of `LedgerStateJudgement`, `MiniProtocolNum`, `PeerRegistry`,
  `PeerSource`, `PeerStatus`, `UseBootstrapPeers`, `UseLedgerPeers`,
  `SocketAddr`, `Duration`, `Instant`)
- 5 `pub mod XXX; pub use XXX::{...};` re-export blocks (40 lines)
- `#[cfg(test)] mod tests;` declaration for the descendant test file

This achieves the upstream-mirror goal for the governor: the file
hierarchy at `crates/network/src/governor/{types,churn,peer_metric,state,counters}.rs`
maps directly onto upstream `Ouroboros.Network.PeerSelection.{Types,
Churn, PeerMetric, Governor, Counters}` (with `Counters` being the
view layer), preserving callers via re-export.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/network/src/governor.rs` | 644 | 76 | −568 |
| `crates/network/src/governor/counters.rs` | (new) | 604 | +604 |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

### Cumulative R270 progress — arc complete

| Slice | File created | Lines moved | governor.rs running size |
|---|---|---|---|
| R270a (Types) | `governor/types.rs` (335) | 342 | 3,146 |
| R270b (Churn) | `governor/churn.rs` (360) | 340 | 2,806 |
| R270c (PeerMetric) | `governor/peer_metric.rs` (466) | 435 | 2,371 |
| R270d (Governor state + evaluators) | `governor/state.rs` (1,778) | 1,727 | 644 |
| **R270e (Counters)** | **`governor/counters.rs` (604)** | **568** | **76** |

**Net governor.rs reduction across R270 arc: 3,488 → 76 lines (−3,412, ~98%).**
The arc successfully splits the monolithic governor.rs into 5 sibling
sub-modules each mirroring a distinct upstream `Ouroboros.Network.PeerSelection.*`
module, while preserving the public re-export surface so callers in
`crates/network/src/runtime.rs`, `crates/network/src/inbound_governor.rs`,
the operational scripts, and `governor/tests.rs` itself continue to
resolve symbols via `crate::governor::...` unchanged.

### Stop point — R270 arc complete; R271 (node/src split) is next per the plan

After R270e, Phase γ moves on to R271: split `node/src/{runtime,sync}.rs`
along upstream `Cardano.Node.*` lines per `docs/REFACTOR_BLUEPRINT.md`.
That arc is structurally similar to R270 (one large file split into
domain-specific sub-modules) but operates in the orchestration layer
rather than the protocol layer.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R270
- R270d closure: `2026-05-06-round-270d-governor-state-extraction.md`
- Upstream peerSelectionStateToView: `.reference-haskell-cardano-node/deps/ouroboros-network/cardano-diffusion/lib/Cardano/Network/PeerSelection/Governor/Types.hs`
- Upstream outboundConnectionsState: same module, `outboundConnectionsState` function
- Upstream simplePeerSelectionPolicy: `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/lib/Ouroboros/Network/Diffusion/Policies.hs`
- Upstream ConnectionManagerCounters: `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network-framework/lib/Ouroboros/Network/ConnectionManager/Types.hs`
