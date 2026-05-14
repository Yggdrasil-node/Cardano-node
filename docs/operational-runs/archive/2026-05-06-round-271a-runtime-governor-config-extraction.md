## Round 271a — `runtime.rs` per-domain split: first slice (RuntimeGovernorConfig)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R271 first slice — `node/src/runtime.rs` per-domain split begins)

### Context

R270 closed Phase γ's network-governor arc (governor.rs 3,488 → 76
lines, −98%, see R270e closure doc). R271 begins the parallel arc for
`node/src/runtime.rs` (7,269 lines) and `node/src/sync.rs` (9,567
lines), splitting them along upstream `Cardano.Node.*` module
boundaries documented in `docs/REFACTOR_BLUEPRINT.md` §Phase D.

R271a extracts `RuntimeGovernorConfig` — the smallest self-contained
configuration struct in runtime.rs — to validate the per-domain split
pattern before tackling larger slices like the mempool helpers,
sync-session bring-up, or RuntimeBlockProducerConfig.

### Slice scope

Extracted 171 source lines from `runtime.rs::RuntimeGovernorConfig`
(struct + impl with 7 builder methods) into a new
`node/src/runtime/governor_config.rs` (191 lines). `runtime.rs` keeps a
`pub mod governor_config;` declaration plus a `pub use
governor_config::RuntimeGovernorConfig;` re-export so existing callers
(`node/src/run_node.rs`, `node/src/main.rs`, the block-producer setup
paths) continue to resolve `crate::runtime::RuntimeGovernorConfig`
unchanged.

The struct mirrors upstream's configuration-overlay layer that
`Cardano.Node.Run.checkPointsAndApplyChunkOptions` builds before
handing off to `peerSelectionGovernor`. Carries:

- Governor cadence: `tick_interval`, `keepalive_interval`,
  `peer_sharing`, `consensus_mode`, `targets`.
- Cross-task shared handles: `block_fetch_pool` (instrumentation),
  `shared_fetch_worker_pool`, `shared_chainsync_worker_pool`,
  `density_registry`.
- Genesis-derived inputs: `ledger_judgement_settings`, `epoch_schedule`.
- Multi-peer tuning: `max_concurrent_block_fetch_peers` knob.

### Visibility model

The `runtime/` directory was previously test-only (`runtime/tests.rs`).
After R271a it now also contains `runtime/governor_config.rs` as a
production submodule. Cross-module references in the new module
resolve via:

- `use yggdrasil_consensus::EpochSchedule;` — external crate, public.
- `use yggdrasil_network::{ConsensusMode, GovernorTargets, NodePeerSharing};` —
  external crate, public.
- `use super::{LedgerJudgementSettings, SharedFetchWorkerPool};` —
  parent runtime.rs's local types. Both are `pub`-level so the
  descendant module resolves them through `super::`.

No visibility promotions were needed. Both `LedgerJudgementSettings`
(at runtime.rs line ~1515) and `SharedFetchWorkerPool` (line 575) are
already `pub` for external use.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `runtime/governor_config.rs::RuntimeGovernorConfig` | upstream configuration overlay built in `Cardano.Node.Run.checkPointsAndApplyChunkOptions` before `peerSelectionGovernor` invocation, plus operator-knob extensions for `bfcMaxConcurrencyBulkSync` (multi-peer BlockFetch) |

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 7,269 | 7,101 | −168 |
| `node/src/runtime/governor_config.rs` | (new) | 191 | +191 |

### Verification gates

```
cargo fmt --all -- --check       # clean (after rustfmt-applied tweak)
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

**Clean first-try extraction** — no fixup passes, no compile errors,
no `#[cfg(test)]` adjustments. The pattern's signature (small bounded
struct with self-contained types + Python-driven extraction with
boundary asserts + automated re-export injection) generalizes well
from the R270 governor experience to the runtime.rs surface.

### Stop point — many more slices to peel from runtime.rs

R271 is multi-round (per dapper plan, 5 days for the full split).
Remaining major candidates per `docs/REFACTOR_BLUEPRINT.md`:

| Round | Target | Approx lines |
|---|---|---|
| R271b | `RuntimeBlockProducerConfig` (struct + impl, line 305+) | ~250 |
| R271c | `LedgerJudgementSettings` (struct + impl, line 1515+) | ~150 |
| R271d | Mempool helpers (`add_tx_to_*` family, lines 3262–3422) | ~200 |
| R271e | `TxSubmissionService*` types (lines 3421–3500) | ~150 |
| R271f | `NodeConfig` + `PeerSession` + `*VerifiedSyncRequest` (lines 3670–3900+) | ~600 |
| R271g | The big sync-session helpers in the second half of runtime.rs | ~2,500 |
| R271h+ | sync.rs split (separate arc — 9,567 lines) | many slices |

After R271 settles, Phase γ moves to R272 (pre-Conway era types split)
and R273 (consensus + plutus + crypto + storage submodule splits).

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- Blueprint: `docs/REFACTOR_BLUEPRINT.md` — Phase D runtime.rs + sync.rs split
- R270 arc closure: `2026-05-06-round-270e-governor-counters-extraction.md`
- Upstream `Cardano.Node.Run`: `.reference-haskell-cardano-node/cardano-node/src/Cardano/Node/Run.hs`
