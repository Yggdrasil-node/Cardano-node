## Round 273b — `nonce.rs` split into `nonce/{derivation,evolution}.rs`

Date: 2026-05-07
Branch: main
Type: Filename-mirror refactor (Phase γ R273 second slice — consensus crate sub-mirrors)

### Slice scope

Split `crates/consensus/src/nonce.rs` (832 lines) into a 448-line
parent `nonce.rs` shell + two new sub-modules:

- `crates/consensus/src/nonce/derivation.rs` (83 lines): VRF-output-to-
  nonce derivation primitives.
- `crates/consensus/src/nonce/evolution.rs` (346 lines): epoch nonce
  evolution state machine (UPDN + TICKN rules).

The residual `nonce.rs` keeps the module-level docstring (UPDN/TICKN
narrative), `pub mod` declarations, `pub use` re-exports of the 6-item
public surface that `crates/consensus/src/lib.rs` re-exports, and the
unchanged `#[cfg(test)] mod tests` block.

### Content distribution

**`nonce/derivation.rs`** — mirrors upstream
`Cardano.Ledger.BaseTypes::hashVerifiedVRF` (TPraos) and
`Ouroboros.Consensus.Protocol.Praos.VRF::vrfNonceValue` (Praos):

- `pub enum NonceDerivation { TPraos, Praos }` — era-aware
  discriminant.
- `pub fn vrf_output_to_nonce` — TPraos: `Blake2b-256(output)`.
- `pub fn praos_vrf_output_to_nonce` — Praos: `Blake2b-256("N" || output)`.
- `pub fn derive_vrf_nonce` — era-aware dispatcher.

**`nonce/evolution.rs`** — mirrors upstream
`Cardano.Protocol.TPraos.Rules.Updn` (UPDN rule) +
`Cardano.Protocol.TPraos.Rules.Tickn` (TICKN rule) +
`Cardano.Protocol.TPraos.API::tickChainDepState` /
`updateChainDepState`:

- `pub struct NonceEvolutionConfig` — per-network/per-era config
  (epoch size, stability window, extra entropy, Byron/Shelley
  transition).
- `pub struct NonceEvolutionState` — per-block mutable state
  (evolving / candidate / epoch / lab / prev-hash nonces).
- `impl NonceEvolutionState` — `apply_block` (UPDN inside-epoch
  update), `from_epoch`, `new`, plus the TICKN epoch-boundary helper
  invoked when `slot_to_epoch(slot) > current_epoch`.
- `impl yggdrasil_ledger::cbor::CborEncode for NonceEvolutionState`
  + decode — chain-dep-state sidecar serialization.
- Private `encode_nonce` / `decode_nonce` helpers.

**`nonce.rs`** (residual) — top-level Nonce shell:

- `pub mod derivation; pub mod evolution;`
- `pub use derivation::{NonceDerivation, derive_vrf_nonce, praos_vrf_output_to_nonce, vrf_output_to_nonce};`
- `pub use evolution::{NonceEvolutionConfig, NonceEvolutionState};`
- `#[cfg(test)] mod tests` — moved unchanged; imports updated to
  pull `EpochSize`, `EpochNo`, `HeaderHash`, `Nonce`, `SlotNo`
  explicitly since `super::*` no longer transitively re-exports them
  via the moved `use` blocks.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `crates/consensus/src/nonce.rs` (shell) | `Cardano.Protocol.TPraos.API::tickChainDepState` / `updateChainDepState` (top-level entry points) |
| `crates/consensus/src/nonce/derivation.rs` | `Cardano.Ledger.BaseTypes::hashVerifiedVRF` (TPraos) + `Ouroboros.Consensus.Protocol.Praos.VRF::vrfNonceValue` (Praos) |
| `crates/consensus/src/nonce/evolution.rs` | `Cardano.Protocol.TPraos.Rules.Updn` (UPDN) + `Cardano.Protocol.TPraos.Rules.Tickn` (TICKN) |

### Cross-module dependencies

- Sub-module `pub use` re-exports preserve the 6-item public surface
  that `crates/consensus/src/lib.rs::pub use nonce::{...}`
  re-exports — no `lib.rs` edits needed.
- `nonce/evolution.rs` reaches `nonce/derivation.rs` via
  `use super::derivation::{NonceDerivation, derive_vrf_nonce};`
  for the per-block VRF-output-to-nonce derivation in
  `apply_block`.

### Visibility / dependency fixups

1. **Test imports** — `nonce.rs::tests::use super::*;` previously
   transitively brought in `EpochSize`, `EpochNo`, `HeaderHash`,
   `Nonce`, `SlotNo` via the file-level `use` blocks. After
   extraction those `use` blocks moved into the sub-modules; the
   tests now import them explicitly via
   `use crate::epoch::EpochSize;` and
   `use yggdrasil_ledger::{EpochNo, HeaderHash, Nonce, SlotNo};`.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/consensus/src/nonce.rs` | 832 | 448 | −384 |
| `crates/consensus/src/nonce/derivation.rs` | (new) | 83 | +83 |
| `crates/consensus/src/nonce/evolution.rs` | (new) | 346 | +346 |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

### Stop point — R273c is the next consensus-crate slice

R273c candidate: split `opcert.rs` (856 lines) along the upstream
`Cardano.Protocol.TPraos.OCert` / `OcertCounterRule` boundary into
`opcert/{cert,counter,rule}.rs` or similar.

Other candidates per the plan: `mempool/queue.rs` (1,665 lines),
`mempool/tx_state.rs` (768 lines), `diffusion_pipelining.rs` (747 lines),
`chain_state.rs` (654 lines).

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R273
- R273a closure: `2026-05-07-round-273a-praos-split.md`
- Upstream UPDN rule:
  `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-protocol-tpraos/src/Cardano/Protocol/TPraos/Rules/Updn.hs`
- Upstream TICKN rule:
  `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-protocol-tpraos/src/Cardano/Protocol/TPraos/Rules/Tickn.hs`
- Upstream Nonce derivation:
  `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/BaseTypes.hs`
