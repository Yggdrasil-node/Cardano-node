## Round 271k — `runtime.rs` per-domain split: eleventh slice (Block-producer slot loop)

Date: 2026-05-07
Branch: main
Type: Filename-mirror refactor (Phase γ R271 eleventh slice — first big async-fn extraction)

### Slice scope

Extracted 463 source lines from `runtime.rs` into a new
`node/src/runtime/block_producer_loop.rs` (503 lines including module
docstring + imports). One item moved:

- `pub async fn run_block_producer_loop<I, V, L, F>(...)` — the
  slot-by-slot leader-check + block-forging async task that runs
  alongside the governor and verified-sync service.

`runtime.rs` keeps a `pub mod block_producer_loop;` declaration plus a
`pub use block_producer_loop::run_block_producer_loop;` re-export so
`run_node.rs` and the runtime entry points continue to resolve
`crate::runtime::run_block_producer_loop` unchanged.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `runtime/block_producer_loop.rs::run_block_producer_loop` | upstream `Ouroboros.Consensus.Node.NodeKernel.forkBlockForging` slot-by-slot leader-check + block-forging async task |

### Cross-module dependencies (no promotions needed)

The function calls 5 private helper fns that stay in `runtime.rs`:

- `super::tip_context_from_chain_db` (line 72)
- `super::mempool_entries_for_forging` (line 93)
- `super::self_validate_forged_block` (line 116)
- `super::kes_expiry_warning` (line 159)
- `super::block_producer_ledger_state_judgement` (line ~1200)

All accessed via explicit `use super::{...};` import in the new module
— no `pub(super)` promotions needed because the descendants-see-private-
ancestors rule lets a child module reference its parent's private
items via `super::`. R271k confirms this pattern works for cross-module
private fn references.

### Visibility / dependency fixups

1. **`yggdrasil_ledger::Point`** — used by the `tip_hash` parameter
   of `make_block_context`; needed an explicit import in the new
   module (just the type, not used by name in any local item).
2. **`yggdrasil_network::LedgerStateJudgement`** — used by
   `block_producer_ledger_state_judgement` return type; explicit import.
3. **External `crate::block_producer::*`** — pulled in 8 names directly:
   `BlockProducerCredentials`, `ShouldForge`, `SlotClock`,
   `assemble_block_body`, `check_should_forge`, `forge_block`,
   `forged_block_to_storage_block`, `make_block_context`.
4. **runtime.rs imports trimmed** — dropped the 7 block-producer-specific
   names (`ShouldForge`, `SlotClock`, `assemble_block_body`,
   `check_should_forge`, `forge_block`, `forged_block_to_storage_block`,
   `make_block_context`) from the runtime.rs `use crate::block_producer::*`
   block since they're no longer referenced in the residual file.
   Kept `BlockProducerCredentials`, `ForgedBlock`,
   `serialize_forged_block_cbor` which other residual fns still use.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `node/src/runtime.rs` | 5,444 | 4,979 | −465 |
| `node/src/runtime/block_producer_loop.rs` | (new) | 503 | +503 |

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
| R271h (KeepAliveScheduler + trace helpers) | `runtime/keep_alive.rs` (~100) | 74 | 5,934 |
| R271i revised (Trace-field builders) | `runtime/tracing.rs` (92) | 55 | 5,879 |
| R271j (ReconnectingRunState cluster) | `runtime/reconnecting.rs` (503) | 435 | 5,444 |
| **R271k (Block-producer slot loop)** | **`runtime/block_producer_loop.rs` (503)** | **465** | **4,979** |

Net `runtime.rs` reduction: **7,269 → 4,979 lines (−2,290, ~31 %)**.
**runtime.rs is now under 5,000 lines for the first time.**

### Stop point — R271l (`run_governor_loop`) is the next slice

R271l will extract `run_governor_loop` (~1,000 lines, currently the
largest single async fn in runtime.rs). Same pattern: extract to
`runtime/governor_loop.rs`, import the runtime.rs-private helpers it
calls via `use super::{...};`. Estimated dependency surface: similar
to or slightly larger than R271k since the governor loop touches more
peer-management and DNS-refresh helpers.

After R271l, what remains in runtime.rs is ~3,500 lines of the 4
`run_reconnecting_verified_sync_service*` family entry points plus
their shared helpers — those split as R271m+ in the final R271 arc
slices.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R271
- R271j closure: `2026-05-07-round-271j-runtime-reconnecting-extraction.md`
- Upstream forkBlockForging: `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Node/NodeKernel.hs`
- Upstream forge tracers: `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Node/Tracers.hs`
