## Round 271s — `runtime.rs` final folds (R271 closeout)

Date: 2026-05-07
Branch: main
Type: Filename-mirror refactor (Phase γ R271 nineteenth slice — final folds)

### Slice scope

Folded the **two remaining residual fns + trait** from `runtime.rs`
into their natural sub-module homes:

1. **`refresh_ledger_peer_sources_from_chain_db`** (~62 lines) →
   `runtime/ledger_peer_source.rs` (the natural home — already houses
   `ChainDbConsensusLedgerSource` and `FilePeerSnapshotSource` that
   the fn instantiates).
2. **`seed_chain_state_via_chain_db` + `trait ChainDbVolatileAccess`
   + 2 trait impls** (~50 lines) → `runtime/reconnecting_sync.rs`
   (the sole consumer — the trait abstracts the two ChainDb access
   modes used by the reconnecting sync entry points).

`runtime.rs` keeps a `pub mod ledger_peer_source;` declaration plus a
`use ledger_peer_source::{block_producer_ledger_state_judgement,
refresh_ledger_peer_sources_from_chain_db};` block bringing the moved
fn into runtime.rs's namespace so `governor_loop.rs`'s 3 super::
references and `tests.rs`'s 1 super:: reference still resolve.

`runtime/reconnecting_sync.rs` drops the
`use super::{ChainDbVolatileAccess, seed_chain_state_via_chain_db, ...}`
imports for the now-local items (kept only the other 17 super::
references which still bind to runtime.rs / sub-module surfaces).

### Mirror mapping

No new mirror mappings — items moved to the modules that already
mirror their upstream concept:

- `ledger_peer_source.rs` mirrors `Ouroboros.Network.Diffusion.LedgerPeers`
  registry refresh; `refresh_ledger_peer_sources_from_chain_db` is the
  runtime-side wrapper that wires the ChainDb-fed `ConsensusLedgerPeerSource`
  + the file-backed `PeerSnapshotFileSource` into
  `live_refresh_ledger_peer_registry_observed`.
- `reconnecting_sync.rs` mirrors `Ouroboros.Consensus.Node.Run.runWith`
  reconnect loop; `ChainDbVolatileAccess` + `seed_chain_state_via_chain_db`
  are the polymorphic helpers used by the four `*_inner` entry points
  to seed post-restart `ChainState` regardless of how the entry point
  holds the ChainDb (`&mut ChainDb` vs `&Arc<RwLock<ChainDb>>`).

### Visibility / dependency fixups

1. **`refresh_ledger_peer_sources_from_chain_db`** now `pub(super)` in
   `ledger_peer_source.rs`. References in the body changed to
   fully-qualify `yggdrasil_network::*` items where they were short-form
   in runtime.rs (since the new module's import block was tighter).
2. **`seed_chain_state_via_chain_db` + `ChainDbVolatileAccess`** now
   `pub(super)` in `reconnecting_sync.rs`. Trait impl blocks for
   `ChainDb<I, V, L>` and `Arc<RwLock<ChainDb<I, V, L>>>` move along
   with the trait.
3. **runtime.rs imports drastically trimmed** — dropped 14 names from
   the top-level `use` blocks: `RwLock`, `NodeTracer`, `trace_fields`,
   `serde_json::json`, `ChainState`, `EpochSchedule`, `SecurityParam`,
   `LedgerState`, `Point`, 6 `yggdrasil_network::*` items, and 4
   `yggdrasil_storage::*` items. The residual file's only remaining
   crate-level `use` is `std::sync::Arc` (for `ChainTipNotify`),
   `crate::sync::LedgerCheckpointTracking` (for the `CheckpointTracking`
   alias), and `#[cfg(test)] crate::sync::VerifiedSyncServiceConfig`
   (for tests.rs).

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 265 | 140 | −125 |
| `node/src/runtime/ledger_peer_source.rs` | 223 | 285 | +62 |
| `node/src/runtime/reconnecting_sync.rs` | 2,065 | 2,119 | +54 |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

### Cumulative R271 progress (final)

| Slice | File created | Lines moved | runtime.rs running size |
|---|---|---|---|
| R271a–r | (18 slices) | 7,004 | 265 |
| **R271s (Final folds)** | (folds, no new files) | **125** | **140** |

Net `runtime.rs` reduction: **7,269 → 140 lines (−7,129, ~98.1 %)**.

**runtime.rs is now a pure orchestration shell — 140 lines of imports,
sub-module declarations, re-export blocks, the `ChainTipNotify` type
alias, the `CheckpointTracking` alias, and `#[cfg(test)] mod tests;`.**

### What remains in runtime.rs (140 lines)

1. ~5 lines: module-level docstring + 1 `use std::sync::Arc;`.
2. ~6 lines: `crate::sync::*` imports for the residual aliases.
3. **~115 lines**: 18 `pub mod ...; pub use ...; use ...;` re-export
   blocks covering all 18 sub-modules under `runtime/`. Test-only
   re-exports gated `#[cfg(test)]`.
4. 1 line: `pub type ChainTipNotify = Arc<tokio::sync::Notify>;`
5. 1 line: `type CheckpointTracking = LedgerCheckpointTracking;`
6. 2 lines: `#[cfg(test)] mod tests;`

### R271 retrospective

Eighteen slices over the R271a–r arc, plus the R271s final fold,
brought `runtime.rs` from a **7,269-line monolith** to a **140-line
orchestration shell**. The arc shipped 19 new sub-modules under
`runtime/`:

| Sub-module | Lines | Slice |
|---|---|---|
| `governor_config.rs` | 191 | R271a |
| `block_producer_config.rs` | 109 | R271b |
| `ledger_judgement.rs` | 45 | R271c |
| `mempool_helpers.rs` | 240 | R271d |
| `tx_submission_service.rs` | 273 | R271e |
| `peer_session.rs` | 421 | R271f |
| `bootstrap.rs` | 188 | R271g |
| `keep_alive.rs` | ~100 | R271h |
| `tracing.rs` | 92 | R271i |
| `reconnecting.rs` | 503 | R271j |
| `block_producer_loop.rs` | 503 | R271k |
| `governor_loop.rs` | 872 | R271l |
| `reconnecting_sync.rs` | 2,119 | R271m + R271s |
| `peer_management.rs` | 923 | R271n |
| `cm_actions.rs` | 352 | R271o |
| `forge.rs` | 151 | R271p |
| `ledger_peer_source.rs` | 285 | R271q + R271s |
| `sync_session.rs` | 495 | R271r |

Patterns confirmed across the arc:

- **Descendants-see-private-ancestors** — child sub-modules can read
  parent runtime.rs's private items via `use super::{...}` without
  any `pub(super)` promotions. R271k–r confirmed across 80+ super::
  references.
- **Item-promotion threshold (~6)** — when a target cluster needs > ~6
  `pub(super)` promotions on parent-private items, extract the shared
  dependency prelude first. R271i (failed, rolled back) → R271i revised
  + R271j (succeeded). Same pattern bit twice in R271n
  (peer_management — required ~25 `pub(super)` promotions because
  three sibling modules consume the cluster).
- **Test-file `super::*` imports** — when `<module>/tests.rs` imports
  symbols via `super::FOO`, moving FOO to a sibling sub-module
  requires runtime.rs to keep `use sibling::FOO;` (or
  `pub use sibling::FOO;`) so the path still resolves. Test-only
  imports gated `#[cfg(test)]`.
- **Orphaned doc comments at extraction boundaries** — when the
  `awk` line range cuts between a doc comment and the item it
  documents, the doc comment is carried into the new file as an
  orphan. R271m, R271n, R271r each hit this once; fixed by
  manually moving the doc inline.

### Stop point — R271 arc complete

R271 finished. Optional follow-up work that could shrink runtime.rs
further (purely cosmetic):

- Move the `ChainTipNotify` type alias to a more appropriate home
  (e.g. `runtime/peer_session.rs` or a new tiny `runtime/types.rs`).
- Merge the `CheckpointTracking` alias into `runtime/sync_session.rs`
  where the alias is most heavily used.

These are not needed; runtime.rs at 140 lines is already a clean
orchestration shell.

### Next R-arc tasks (per the plan)

- R269r-style Conway-rule sub-mirror under `state/eras/conway/rules/`
  (~3 days, 19 substantive .rs files mirroring upstream).
- R272 pre-Conway era rules split (5 days).
- R273 consensus + plutus + crypto + storage submodule splits (3 days).
- R266 step 3 Gap BP per-builtin trace comparison (operator-time).
- R267 mainnet 24h+ endurance (operator-time).
- R268 naming-parity sweep (3 days).
- R274 trace forwarder (3 days).
- R275 1.0 sign-off (operator-time).

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271r closure: `2026-05-07-round-271r-runtime-sync-session-extraction.md`
- Upstream Run.runWith reconnect loop:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Node/Run.hs`
- Upstream LedgerPeers refresh:
  `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/src/Ouroboros/Network/Diffusion/LedgerPeers.hs`
