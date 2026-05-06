## Round 270a — `governor.rs` per-domain split: first slice (Types)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R270 first slice — `Ouroboros.Network.PeerSelection.*` mirror)

### Context

R269 series completed Phase γ's per-state/per-era arc (see
`2026-05-06-round-269w-state-eras-conway-extraction.md`). R270 begins
the parallel arc for `crates/network/src/governor.rs` (3,488 lines),
splitting it along upstream `Ouroboros.Network.PeerSelection.*` module
boundaries.

R270a extracts the **Types** sub-module (governor targets, peer-sharing
mode, association mode, sensitive-mode predicates) — the smallest
self-contained slice — to validate the per-domain split pattern before
tackling the larger Churn / PeerMetric / Governor (~2,500-line bulk)
slices.

### Slice scope

Extracted 316 source lines from `governor.rs` (lines 53–368) into a new
`crates/network/src/governor/types.rs` (335 lines including module
docstring + imports). Items moved:

- `pub struct GovernorTargets` + 2 impls (sane-target predicate +
  Default).
- `pub struct LocalRootTargets` + impl (build from local-root config).
- `pub enum PeerSelectionMode` + helper fns
  `requires_bootstrap_peers`, `peer_selection_mode`,
  `is_node_able_to_make_progress`.
- `pub enum NodePeerSharing` + impl (wire codec + is_enabled).
- `pub enum AssociationMode` + helper fn `compute_association_mode`.

`governor.rs` keeps a `pub mod types;` declaration plus a `pub use
types::{…};` re-export block so existing callers using
`crate::governor::PeerSelectionMode` etc. continue to resolve unchanged.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `governor/types.rs::GovernorTargets` | upstream `PeerSelectionTargets` in `Ouroboros.Network.PeerSelection.Governor.Types` |
| `governor/types.rs::PeerSelectionMode` | upstream sensitive/normal mode flags from `Cardano.Network.PeerSelection.Bootstrap` |
| `governor/types.rs::NodePeerSharing` | upstream `PeerSharing` in `Ouroboros.Network.PeerSelection.PeerSharing` |
| `governor/types.rs::AssociationMode` | upstream `AssociationMode` + `readAssociationMode` from `Ouroboros.Network.PeerSelection.Governor.Monitor` |

### Edit-tooling note: derives + doc-comments are external to the struct line

A subtle bug in this round's Python extractor: when extracting from
"line N where `pub struct X {` lives" through "line M", the
`#[derive(...)]` attribute and accompanying doc-comment block sit on
lines N-K (immediately above the struct) and were initially left in
governor.rs as orphans. Fixed by:

1. Deleting the orphan section divider + doc + derive from
   `governor.rs` after the extraction.
2. Confirming each extracted struct/enum had its derive carried
   alongside (each item except the first had its derive *inside* the
   extracted range; only `GovernorTargets` had its derive at the
   line *immediately preceding* the captured range, which became the
   orphan).

Lesson: any future per-module extraction should grow the boundary
backward to include doc-comments + attributes (`#[derive]`,
`#[allow]`, etc.) that decorate the first item, not just the item's
keyword line.

### Visibility note: `#[cfg(test)] use` for descendants of removed imports

Removing `UseLedgerPeers` from `governor.rs` broke `governor/tests.rs`
(which is a descendant module declared via `#[cfg(test)] mod tests;`
at the bottom of governor.rs and accessed `UseLedgerPeers` via
descendant inheritance). Fixed by adding `#[cfg(test)] use
crate::root_peers::UseLedgerPeers;` to governor.rs — keeps the symbol
visible only in test builds where governor's child tests need it.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/network/src/governor.rs` | 3,488 | 3,146 | −342 |
| `crates/network/src/governor/types.rs` | (new) | 335 | +335 |

The `−7` net is a small loss from collapsing the section-divider
comments and orphan derive into types.rs's module docstring.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

### Stop point — Churn (R270b) + PeerMetric (R270c) follow

Remaining domains in `governor.rs`:

| Round | Mirror target | Lines (approx) |
|---|---|---|
| R270b | `Ouroboros.Network.PeerSelection.Churn` (`ChurnPhase`, `ChurnConfig`, `ChurnMode`, `ChurnRegime`, `churn_decrease`, `pick_churn_regime`, etc.) | ~250 |
| R270c | `Ouroboros.Network.PeerSelection.PeerMetric` (`PeerMetrics`, `HotPeerScheduling`, `PeerFailureRecord`, `RequestBackoffState`, `PickPolicy`, `Xorshift64`) | ~430 |
| R270d | `Ouroboros.Network.PeerSelection.Governor` (`GovernorState`, `GovernorAction`, all `evaluate_*` functions, `governor_tick`) | ~1,800 (the bulk) |
| R270e | `Ouroboros.Network.PeerSelection.{Counters, ConnectionManager}` (`PeerSelectionCounters`, `OutboundConnectionsState`, `ConnectionManagerCounters`, `PeerSelectionTimeouts`, `FetchMode`) | ~600 |

R270b is the natural next slice — small, well-bounded, follows the
same pattern.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R270
- R269 closure (per-state arc): `2026-05-06-round-269w-state-eras-conway-extraction.md`
- Upstream targets: `.reference-haskell-cardano-node/deps/ouroboros-network/cardano-diffusion/lib/Cardano/Network/PeerSelection/Governor/Types.hs`
- Upstream peer-sharing: `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/lib/Ouroboros/Network/PeerSelection/PeerSharing.hs`
- Upstream sensitive-mode bootstrap predicate: `.reference-haskell-cardano-node/deps/ouroboros-network/cardano-diffusion/api/lib/Cardano/Network/PeerSelection/Bootstrap.hs`
