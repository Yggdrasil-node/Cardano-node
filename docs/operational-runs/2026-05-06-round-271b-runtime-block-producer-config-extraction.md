## Round 271b — `runtime.rs` per-domain split: second slice (Block-producer config + shared state)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R271 second slice)

### Slice scope

Bundled three block-producer-related items from `runtime.rs` into a
new `node/src/runtime/block_producer_config.rs` (109 lines):

- `SharedBlockProducerState` (struct) — live epoch-nonce + per-pool
  stake-sigma values written by the sync pipeline so the block-producer
  loop reads them concurrently without polling.
- `update_bp_state_nonce` — updater called after each sync batch
  applies nonce evolution.
- `update_bp_state_sigma` — updater called after each sync batch
  rotates stake snapshots.
- `RuntimeBlockProducerConfig` (struct) — immutable startup-time
  configuration: slot length, system start, max ledger age, active
  slot coefficient, KES expiry warning thresholds, max block body
  size, protocol version.

The two updater fns were `fn` (private to runtime.rs); promoted to
`pub fn` in the new module so the re-export at runtime.rs makes them
reachable from sync.rs and run_node.rs callers.

`runtime.rs` keeps a `pub mod block_producer_config;` declaration
(placed alongside the R271a `governor_config` mod) plus a `pub use
block_producer_config::{…};` re-export listing all 4 items so existing
callers continue to resolve `crate::runtime::SharedBlockProducerState`
etc. unchanged.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `runtime/block_producer_config.rs::RuntimeBlockProducerConfig` | upstream `Ouroboros.Consensus.Node.Forking.forkBlockForging` runtime configuration record (slot length, system start, max ledger age, KES expiry, max block body size, protocol version) |
| `runtime/block_producer_config.rs::SharedBlockProducerState` + `update_bp_state_nonce` + `update_bp_state_sigma` | upstream `Ouroboros.Consensus.Node.Forking.SharedKernelState` per-slot live inputs (epoch nonce + per-pool relative stake sigma re-read on every slot tick) |

### Edit-tooling note: two-chunk extraction with intermediate non-deletable content

`SharedBlockProducerState` and `RuntimeBlockProducerConfig` are
bracketed by the R271a `pub mod governor_config;` mod declaration at
runtime.rs lines 131–132 (which must stay where it is).

The Python extractor handled this by:
1. Identifying both chunks separately by their unique doc-comment
   headers (`/// Shared block-producer state` and `/// Runtime
   block-producer configuration`).
2. Concatenating both into the destination file.
3. Deleting the higher-numbered chunk first (so the lower-numbered
   chunk's indices remain valid for the second deletion).
4. Locating the surviving `pub mod governor_config;` line and inserting
   the new `pub mod block_producer_config; pub use ...;` block right
   above it.

This generalizes the R270 single-chunk extraction pattern to
multi-chunk slices with anchor preservation, useful for future runtime
slices where the targets are interleaved with already-extracted items.

### Visibility / import fixups

1. **`Nonce` source.** Initially imported from `yggdrasil_crypto::Nonce`
   (incorrect — Yggdrasil's `Nonce` lives at `yggdrasil_ledger::Nonce`,
   re-exported from the consensus crate via the ledger crate). Fixed
   to `use yggdrasil_ledger::Nonce;`.
2. **`StakeSnapshots` redundant import.** The updater fn references
   `&yggdrasil_ledger::StakeSnapshots` via full path; the named
   import was unused and removed.
3. **Updater fns promotion.** Both `update_bp_state_nonce` and
   `update_bp_state_sigma` were `fn` (module-private). Promoted to
   `pub fn` so the runtime.rs re-export brings them into the standard
   `crate::runtime::*` resolution scope.
4. **runtime.rs import slimming.** Dropped now-unused
   `yggdrasil_consensus::praos::ActiveSlotCoeff` (used only by the
   moved `RuntimeBlockProducerConfig`) and `Nonce` (used only by the
   moved `SharedBlockProducerState`). The remaining
   `yggdrasil_ledger::*` import block kept the rest of the imports it
   was already using.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 7,101 | 7,020 | −81 |
| `node/src/runtime/block_producer_config.rs` | (new) | 109 | +109 |

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
| **R271b (Block-producer config + state)** | **`runtime/block_producer_config.rs` (109)** | **81** | **7,020** |

Net `runtime.rs` reduction so far: **7,269 → 7,020 lines (−249, ~3 %)**.

### Stop point — R271c (LedgerJudgementSettings) is the next slice

Remaining major candidates per `docs/REFACTOR_BLUEPRINT.md`:

| Round | Target | Approx lines |
|---|---|---|
| R271c | `LedgerJudgementSettings` (struct + impl, line ~1346+) | ~150 |
| R271d | Mempool helpers (`add_tx_to_*` family) | ~200 |
| R271e | `TxSubmissionService*` types | ~150 |
| R271f | `NodeConfig` + `PeerSession` + sync-request types | ~600 |
| R271g | Big sync-session helpers in second half | ~2,500 |
| R271h+ | sync.rs split (separate arc, 9,567 lines) | many slices |

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271a closure: `2026-05-06-round-271a-runtime-governor-config-extraction.md`
- Upstream `forkBlockForging` + `SharedKernelState`: `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Node/Forking.hs`
