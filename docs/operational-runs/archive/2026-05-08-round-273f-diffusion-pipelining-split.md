## Round 273f — `diffusion_pipelining.rs` split into `diffusion_pipelining/{identity,state}.rs`

Date: 2026-05-08
Branch: main
Type: Filename-mirror refactor (Phase γ R273 sixth slice — consensus crate sub-mirrors)

### Slice scope

Split `crates/consensus/src/diffusion_pipelining.rs` (747 lines) into a
291-line parent `diffusion_pipelining.rs` shell + two new sub-modules:

- `crates/consensus/src/diffusion_pipelining/identity.rs` (230 lines):
  per-pool tentative-header tracking (`HotIdentity`,
  `TentativeHeaderView`, `TentativeHeaderState`).
- `crates/consensus/src/diffusion_pipelining/state.rs` (282 lines):
  diffusion-pipelining state machine (`DiffusionPipeliningSupport`,
  `TentativeState`, `TentativeHeader`, `PipeliningEvent`,
  `PeerPipeliningState`).

The residual `diffusion_pipelining.rs` keeps the module-level
docstring, `pub mod` declarations, `pub use` re-exports of the 8-item
public surface that `crates/consensus/src/lib.rs` re-exports, and the
unchanged tests block.

### Content distribution

**`diffusion_pipelining/identity.rs`** — mirrors upstream
`Ouroboros.Consensus.Shelley.Node.DiffusionPipelining`:

- `pub struct HotIdentity` — block issuer identity (Blake2b-224 hash
  of cold key + opcert sequence number) used by the safety criterion.
- `impl HotIdentity` — `from_parts`, `from_block_issuer_vkey`.
- `pub struct TentativeHeaderView` — (`block_no`, `identity`) projection
  used by `apply_tentative_header_view`.
- `impl TentativeHeaderView::from_header_body`.
- `pub struct TentativeHeaderState` — per-pool ring tracking which
  block numbers have been pipelined (and which were trap headers).
  `last_trap_block_no` and `bad_identities` fields promoted to
  `pub(super)` so the test module can inspect them directly.
- `impl TentativeHeaderState` — `initial`, `apply_tentative_header_view`,
  plus accessor methods.
- `impl Default for TentativeHeaderState`.

**`diffusion_pipelining/state.rs`** — mirrors upstream
`Ouroboros.Consensus.Block.SupportsDiffusionPipelining` +
`HardFork.Combinator.Node.DiffusionPipelining`:

- `pub enum DiffusionPipeliningSupport` — feature flag
  (`DiffusionPipeliningOff` / `DiffusionPipeliningOn`).
- `pub struct TentativeState` — global tentative-tip orchestrator.
- `pub struct TentativeHeader` — the currently-pipelined tentative
  header.
- `pub enum PipeliningEvent` — per-event trace surface (announced,
  retracted, confirmed, trap detected).
- `pub struct PeerPipeliningState` — per-peer subset used by the
  inbound ChainSync server.
- `impl TentativeState` (with `Default` + 8 methods covering
  set/clear/event-emission semantics).
- `impl PeerPipeliningState` (with `Default`).

state.rs reaches identity.rs via
`use super::identity::{TentativeHeaderState, TentativeHeaderView};`.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `crates/consensus/src/diffusion_pipelining.rs` (shell) | `Ouroboros.Consensus.Block.SupportsDiffusionPipelining` (top-level) |
| `crates/consensus/src/diffusion_pipelining/identity.rs` | `Ouroboros.Consensus.Shelley.Node.DiffusionPipelining` (`HotIdentity` + per-pool tentative-header state) |
| `crates/consensus/src/diffusion_pipelining/state.rs` | `Ouroboros.Consensus.HardFork.Combinator.Node.DiffusionPipelining` + `Block.SupportsDiffusionPipelining::DiffusionPipeliningSupport` |

### Cross-module dependencies

- 2 fields on `TentativeHeaderState` (`last_trap_block_no`,
  `bad_identities`) promoted to `pub(super)` so the parent's test
  module can inspect them via `super::*`.
- state.rs imports the 2 identity types it composes via
  `use super::identity::{TentativeHeaderState, TentativeHeaderView};`.
- 8-item public surface preserved unchanged via sub-module
  `pub use` re-exports — no `lib.rs` edits needed.

### Visibility / dependency fixups

1. **Test imports** — tests `use super::*;` previously transitively
   pulled `VerificationKey` and `hash_bytes_224`; now imported
   explicitly via `use yggdrasil_crypto::{blake2b::hash_bytes_224,
   ed25519::VerificationKey};`.
2. **`TentativeHeaderState` field promotions** — `last_trap_block_no`
   and `bad_identities` fields are inspected directly by the test
   module's `higher_block_no_always_resets` and
   `multiple_issuers_tracked_at_same_block_no` tests.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/consensus/src/diffusion_pipelining.rs` | 747 | 291 | −456 |
| `crates/consensus/src/diffusion_pipelining/identity.rs` | (new) | 230 | +230 |
| `crates/consensus/src/diffusion_pipelining/state.rs` | (new) | 282 | +282 |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

### Cumulative R273 progress

| Slice | Files moved/created | Source file Δ |
|---|---|---|
| R273a (praos) | `praos/{vrf,common}.rs` | 793 → 464 (−329) |
| R273b (nonce) | `nonce/{derivation,evolution}.rs` | 832 → 448 (−384) |
| R273c (opcert) | `opcert/{cert,counter}.rs` | 856 → 547 (−309) |
| R273d (mempool/queue) | `queue/{inner,shared}.rs` | 1,665 → 731 (−934) |
| R273e (mempool/tx_state) | `tx_state/{state,shared}.rs` | 768 → 319 (−449) |
| **R273f (diffusion_pipelining)** | **`diffusion_pipelining/{identity,state}.rs`** | **747 → 291 (−456)** |

Total moved: ~2,861 lines across 12 sub-modules.

### Stop point — R273g is the next slice

Remaining ≥600-line consensus files:

| File | Lines | Likely split |
|---|---|---|
| `crates/consensus/src/chain_state.rs` | 654 | volatile / immutable / dep-state split |

Plus the plutus crate's >1k-line monoliths (`cost_model.rs` 1,718,
`types.rs` 1,707, `builtins.rs` 1,483, `machine.rs` 1,460, `flat.rs`
1,245) and crypto (`vrf.rs` 1,254, `sum_kes.rs` 1,018, `kes.rs` 939).

R273g candidate: `chain_state.rs` (654 lines) — the last
≥600-line consensus file that hasn't been split yet.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R273
- R273e closure: `2026-05-08-round-273e-mempool-tx-state-split.md`
- Upstream SupportsDiffusionPipelining:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Block/SupportsDiffusionPipelining.hs`
- Upstream Shelley DiffusionPipelining:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/shelley/Ouroboros/Consensus/Shelley/Node/DiffusionPipelining.hs`
- Upstream HardFork DiffusionPipelining:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/HardFork/Combinator/Node/DiffusionPipelining.hs`
