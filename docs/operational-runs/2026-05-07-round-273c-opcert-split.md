## Round 273c — `opcert.rs` split into `opcert/{cert,counter}.rs`

Date: 2026-05-07
Branch: main
Type: Filename-mirror refactor (Phase γ R273 third slice — consensus crate sub-mirrors)

### Slice scope

Split `crates/consensus/src/opcert.rs` (856 lines) into a 547-line
parent `opcert.rs` shell + two new sub-modules:

- `crates/consensus/src/opcert/cert.rs` (107 lines): OpCert struct +
  KES period helpers.
- `crates/consensus/src/opcert/counter.rs` (246 lines): OcertCounters
  + OcertCounterRule + CBOR encode/decode.

The residual `opcert.rs` keeps the module-level docstring,
`pub mod` declarations, `pub use` re-exports of the 5-item public
surface that `crates/consensus/src/lib.rs` re-exports, and the
unchanged `#[cfg(test)] mod tests` block.

### Content distribution

**`opcert/cert.rs`** — mirrors upstream
`Cardano.Protocol.TPraos.OCert::OCert` and the `kesPeriod` /
`checkKESPeriod` helpers in the same module:

- `pub struct OpCert` — operational certificate binding cold key →
  hot KES key (`hot_vkey`, `sequence_number`, `kes_period`,
  cold-key `signature`).
- `impl OpCert::verify` — verifies the cold-key signature over the
  canonical signable representation.
- `pub fn kes_period_of_slot` — `slot / slots_per_kes_period` with
  safe div-by-zero check.
- `pub fn check_kes_period` — verify current KES period falls within
  the certificate's validity window.

**`opcert/counter.rs`** — mirrors upstream
`Cardano.Protocol.TPraos.Rules.OCert` (TPraos counter rule, Shelley/
Allegra/Mary/pre-Babbage) and `Ouroboros.Consensus.Protocol.Praos`
(Praos counter rule, Babbage+ Vasil HF onward):

- `pub struct OcertCounters` — per-pool monotonic counter map.
- `pub enum OcertCounterRule { TPraos, Praos }` + `for_pv_major`
  protocol-version dispatcher.
- `impl OcertCounters` — `validate_and_update`, `current_no`, plus
  pool initialisation rules (lookup in stake distribution +
  `NoCounterForKeyHash` error path).
- `impl yggdrasil_ledger::cbor::CborEncode for OcertCounters` +
  `CborDecode` — chain-dep-state sidecar serialisation.

**`opcert.rs`** (residual) — top-level OpCert shell:

- `pub mod cert; pub mod counter;`
- `pub use cert::{OpCert, check_kes_period, kes_period_of_slot};`
- `pub use counter::{OcertCounterRule, OcertCounters};`
- `#[cfg(test)] mod tests` — moved unchanged; imports updated to
  pull `ConsensusError`, `Signature`, `VerificationKey`,
  `SumKesVerificationKey` explicitly since `super::*` no longer
  transitively re-exports them via the moved `use` blocks.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `crates/consensus/src/opcert.rs` (shell) | `Cardano.Protocol.TPraos.OCert` (top-level module) |
| `crates/consensus/src/opcert/cert.rs` | `Cardano.Protocol.TPraos.OCert::OCert` + `kesPeriod` / `checkKESPeriod` |
| `crates/consensus/src/opcert/counter.rs` | `Cardano.Protocol.TPraos.Rules.OCert` (TPraos) + `Ouroboros.Consensus.Protocol.Praos` (Praos counter rule) |

### Cross-module dependencies

- Sub-module `pub use` re-exports preserve the 5-item public surface
  that `crates/consensus/src/lib.rs::pub use opcert::{...}`
  re-exports — no `lib.rs` edits needed.
- The two sub-modules are independent — `cert.rs` and `counter.rs`
  don't reference each other directly.

### Visibility / dependency fixups

1. **Test imports** — `opcert.rs::tests::use super::*;` previously
   transitively brought in `ConsensusError`, `Signature`,
   `VerificationKey`, `SumKesVerificationKey` via the file-level
   `use` blocks. After extraction the tests now import them
   explicitly via
   `use crate::error::ConsensusError;` and
   `use yggdrasil_crypto::ed25519::{Signature, SigningKey, VerificationKey};` and
   `use yggdrasil_crypto::sum_kes::SumKesVerificationKey;`.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/consensus/src/opcert.rs` | 856 | 547 | −309 |
| `crates/consensus/src/opcert/cert.rs` | (new) | 107 | +107 |
| `crates/consensus/src/opcert/counter.rs` | (new) | 246 | +246 |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

### Cumulative R273 progress

| Slice | File created | Lines moved | Source file Δ |
|---|---|---|---|
| R273a (praos) | `praos/{vrf,common}.rs` | 396 | 793 → 464 (−329) |
| R273b (nonce) | `nonce/{derivation,evolution}.rs` | 429 | 832 → 448 (−384) |
| **R273c (opcert)** | **`opcert/{cert,counter}.rs`** | **353** | **856 → 547 (−309)** |

### Stop point — R273d is the next consensus-crate slice

R273d candidates per the plan:

| File | Lines | Likely split |
|---|---|---|
| `crates/consensus/src/mempool/queue.rs` | 1,665 | per-policy / per-bucket |
| `crates/consensus/src/mempool/tx_state.rs` | 768 | per-state machine |
| `crates/consensus/src/diffusion_pipelining.rs` | 747 | per-state-machine slice |
| `crates/consensus/src/chain_state.rs` | 654 | volatile vs immutable |

R273d candidate: split `chain_state.rs` (654 lines) since it's a
manageable size and structurally similar to nonce/opcert (cluster of
struct + impls + helpers).

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R273
- R273b closure: `2026-05-07-round-273b-nonce-split.md`
- Upstream OCert struct + helpers:
  `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-protocol-tpraos/src/Cardano/Protocol/TPraos/OCert.hs`
- Upstream OCERT counter rule (TPraos):
  `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-protocol-tpraos/src/Cardano/Protocol/TPraos/Rules/OCert.hs`
- Upstream Praos counter rule:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/Praos.hs`
