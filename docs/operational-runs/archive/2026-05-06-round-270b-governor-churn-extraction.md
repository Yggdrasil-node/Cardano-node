## Round 270b — `governor.rs` per-domain split: second slice (Churn + Fetch mode)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R270 second slice)

### Slice scope

Bundled both Churn-related clusters from `governor.rs` into a single
new `crates/network/src/governor/churn.rs` (360 lines):

**Cluster 1 (lines 33–142, 109 lines):**
- `ChurnPhase` enum (two-phase decrease cycle marker)
- `ChurnConfig` struct + `Default` impl + helper impl
- `churn_decrease(count: usize) -> usize` — the upstream `decrease v = max 0 (v - max 1 (v/5))` formula

**Cluster 2 (lines 2716–2950, 234 lines):**
- `FetchMode` enum (`FetchModeBulkSync`, `FetchModeDeadline`)
- `fetch_mode_from_judgement(LedgerStateJudgement) -> FetchMode`
- `ChurnMode` enum + `churn_mode_from_fetch_mode`
- `ConsensusMode` enum (Praos vs Genesis)
- `ChurnRegime` enum (`ChurnDefault`, `ChurnBootstrapPraosSync`, `ChurnPraosSync`)
- `pick_churn_regime(...)` — combines fetch mode + bootstrap flag + consensus mode
- `churn_decrease_active`, `churn_decrease_established` — regime-aware variants

`governor.rs` keeps a `pub mod churn;` declaration plus a `pub use
churn::{…};` re-export block.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `governor/churn.rs::ChurnPhase` / `ChurnConfig` / `churn_decrease` | upstream `Ouroboros.Network.PeerSelection.Churn` `peerChurnGovernor` two-phase decrease/restore loop |
| `governor/churn.rs::FetchMode` + `fetch_mode_from_judgement` | upstream `Ouroboros.Network.BlockFetch.ConsensusInterface` `FetchMode` + `Cardano.Node.Diffusion.mkReadFetchMode` |
| `governor/churn.rs::ChurnMode` / `ChurnRegime` / `pick_churn_regime` | upstream `Ouroboros.Network.PeerSelection.Churn` regime selection (formerly inline in `peerChurnGovernor`) |

Pragmatic-mirror choice: bundling `FetchMode` (technically from
`BlockFetch.ConsensusInterface`) into `governor/churn.rs` because the
downstream consumers in yggdrasil are exclusively churn-related. A
strict 1:1 mirror would split it into `governor/fetch_mode.rs` or
`crates/network/src/blockfetch/consensus_interface.rs`; per the plan's
pragmatic-mirror rules, bundle small adjacent concerns and split only
when downstream usage diverges.

### Visibility note (recurring pattern, also in R270a)

Removing the `LedgerStateJudgement` import from `governor.rs` after
moving `FetchMode` broke `governor/tests.rs` (descendant module via
`#[cfg(test)] mod tests;`) which uses `LedgerStateJudgement` directly.
Fixed by re-adding `#[cfg(test)] use crate::ledger_peers_provider::LedgerStateJudgement;`
to governor.rs — keeps the symbol visible only in test builds.

This is the same pattern as R270a's `UseLedgerPeers` fix. The
`#[cfg(test)] use` re-add is becoming a recurring step for any future
governor sub-module extraction that removes lib-side dependencies the
tests still rely on. Worth noting for R270c onwards.

### Edit-tooling note: search-by-regex for cluster boundaries

This round used regex search (`re.match(r'^pub enum FetchMode\b', ...)`)
to locate cluster 2 boundaries dynamically, since rustfmt may shift
exact line numbers between Read calls and Python execution. Walking
back from the `pub enum` line through preceding `///` doc comments and
blank lines until hitting a `// ---` section divider correctly captures
the full doc-block + derive + item span without hardcoding offsets.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/network/src/governor.rs` | 3,146 | 2,806 | −340 |
| `crates/network/src/governor/churn.rs` | (new) | 360 | +360 |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

### Stop point — R270c (PeerMetric) is the next slice

Remaining domains in `governor.rs`:

| Round | Mirror target | Approx lines |
|---|---|---|
| R270c | `Ouroboros.Network.PeerSelection.PeerMetric` (`PeerMetrics`, `HotPeerScheduling`, `PeerFailureRecord`, `RequestBackoffState`, `PickPolicy`, `Xorshift64`) | ~430 |
| R270d | `Ouroboros.Network.PeerSelection.Governor` (`GovernorState`, `GovernorAction`, all `evaluate_*` functions, `governor_tick`) | ~1,800 (the bulk) |
| R270e | `Ouroboros.Network.PeerSelection.{Counters, ConnectionManager}` (`PeerSelectionCounters`, `OutboundConnectionsState`, `ConnectionManagerCounters`, `PeerSelectionTimeouts`) | ~600 |

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R270
- R270a closure: `2026-05-06-round-270a-governor-types-extraction.md`
- Upstream Churn governor: `.reference-haskell-cardano-node/deps/ouroboros-network/cardano-diffusion/lib/Cardano/Network/PeerSelection/Churn.hs`
- Upstream BlockFetch FetchMode: `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/lib/Ouroboros/Network/BlockFetch/ConsensusInterface.hs`
