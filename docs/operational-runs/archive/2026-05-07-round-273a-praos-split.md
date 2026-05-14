## Round 273a — `praos.rs` split into `praos/{vrf,common}.rs`

Date: 2026-05-07
Branch: main
Type: Filename-mirror refactor (Phase γ R273 first slice — consensus crate sub-mirrors)

### Slice scope

Split `crates/consensus/src/praos.rs` (793 lines) into a 464-line
parent `praos.rs` shell + two new sub-modules:

- `crates/consensus/src/praos/vrf.rs` (122 lines): VRF input
  construction.
- `crates/consensus/src/praos/common.rs` (274 lines): `ActiveSlotCoeff`
  and Taylor-series math primitives.

The residual `praos.rs` keeps the leader-check entry points
(`check_leader_value`, `check_is_leader`, `verify_leader_proof`,
`verify_leader_proof_output`, `verify_nonce_proof`) plus the
end-of-file `#[cfg(test)] mod tests` block.

### Content distribution

**`praos/vrf.rs`** — mirrors upstream
`Ouroboros.Consensus.Protocol.Praos.VRF::mkInputVRF` and
`Cardano.Protocol.TPraos.BHeader::mkSeed`:

- `pub enum VrfMode { TPraos, Praos }`
- `pub enum VrfUsage { Leader, Nonce }`
- `pub fn praos_vrf_input` — Babbage/Conway VRF input
  (Blake2b-256 over `slot_be8 || nonce`).
- `pub fn tpraos_vrf_seed` — Shelley/Allegra/Mary/Alonzo VRF seed
  (Blake2b-256 over `slot_be8 || nonce` XOR per-purpose tag hash).
- `pub fn vrf_input` — mode-aware dispatcher.
- `pub(super) fn raw_vrf_input_bytes` — pre-hash concatenation, used
  internally and by parent `praos.rs` test module.
- `pub(super) fn tpraos_seed_tag_hash` — per-purpose tag hash, used
  internally and by parent `praos.rs` test module.

**`praos/common.rs`** — mirrors upstream
`Cardano.Ledger.BaseTypes::ActiveSlotCoeff` and the `taylorExpCmp`
helper in `Ouroboros.Consensus.Protocol.Praos.VRF`:

- `pub struct ActiveSlotCoeff` (3 fields, `f_val` private,
  `log_num`/`log_den` `pub(super)` for `check_leader_value` access).
- `impl ActiveSlotCoeff` — `from_rational`, `new`, `to_f64`.
- `impl PartialEq for ActiveSlotCoeff`.
- `pub fn leadership_threshold` — diagnostic floating-point
  `phi_f(sigma) = 1 - (1 - f)^sigma`.
- `pub(super) fn taylor_exp_cmp` — Taylor-series comparison of
  `target` vs `q * exp(-x)` over a rational `x`. Used by parent's
  `check_leader_value`.
- Private: `LN_SERIES_TERMS`, `EXP_SERIES_TERMS`,
  `compute_neg_ln_one_minus`, `gcd_u64`.

**`praos.rs`** (residual) — top-level Praos protocol entry points:

- `pub use common::{ActiveSlotCoeff, leadership_threshold};`
- `pub use vrf::{VrfMode, VrfUsage, praos_vrf_input, tpraos_vrf_seed, vrf_input};`
- `pub fn check_leader_value` — leader-value threshold check on a
  known VRF output.
- `pub fn check_is_leader` — full pipeline: VRF proof + threshold check.
- `pub fn verify_leader_proof` / `verify_leader_proof_output` —
  verifier-side leader VRF proof check.
- `pub fn verify_nonce_proof` — verifier-side nonce VRF proof check.
- `#[cfg(test)] mod tests` — moved unchanged.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `crates/consensus/src/praos.rs` (entry points) | `Ouroboros.Consensus.Protocol.Praos` |
| `crates/consensus/src/praos/vrf.rs` | `Ouroboros.Consensus.Protocol.Praos.VRF` (`mkInputVRF`) + `Cardano.Protocol.TPraos.BHeader::mkSeed` |
| `crates/consensus/src/praos/common.rs` | `Cardano.Ledger.BaseTypes::ActiveSlotCoeff` + `taylorExpCmp` |

### Cross-module dependencies

- 6 items promoted to `pub(super)` (2 helper fns in vrf.rs,
  2 fields + 1 fn in common.rs, plus 2 cfg-gated test imports
  back into parent praos.rs).
- Sub-module `pub use` re-exports preserve the 12-item public
  surface that `crates/consensus/src/lib.rs::pub use praos::{...}`
  re-exports to crate consumers — no `lib.rs` edits needed.

### Visibility / dependency fixups

1. **`raw_vrf_input_bytes`, `tpraos_seed_tag_hash`** → `pub(super)`
   in vrf.rs because parent praos.rs's test module uses them
   directly. Imported via
   `#[cfg(test)] use vrf::{raw_vrf_input_bytes, tpraos_seed_tag_hash};`
   in praos.rs.
2. **`ActiveSlotCoeff::log_num`, `log_den`** → `pub(super)` in
   common.rs because parent praos.rs's `check_leader_value`
   constructs `BigUint::from(sigma_num) * &active_slot_coeff.log_num`
   directly.
3. **`taylor_exp_cmp`** → `pub(super)` in common.rs because parent
   `check_leader_value` invokes it as the inner comparator.
4. **Orphaned doc comment for `compute_neg_ln_one_minus`** —
   the bulk-extract carried 2 lines of doc above the moved fn into
   the residual praos.rs around the section-header. Removed inline
   to avoid clippy `empty_line_after_doc_comment`.
5. **Wrapped-comment blank line in
   `verify_leader_proof`** — fixed a `//` block that had an
   accidental blank line in the middle (clippy
   `empty_line_after_doc_comment`).

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/consensus/src/praos.rs` | 793 | 464 | −329 |
| `crates/consensus/src/praos/vrf.rs` | (new) | 122 | +122 |
| `crates/consensus/src/praos/common.rs` | (new) | 274 | +274 |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

### Stop point — R273b is the next consensus-crate slice

R273 covers consensus + plutus + crypto + storage submodule splits.
After R273a, the consensus crate's biggest remaining single-file
monoliths are:

| File | Lines | Candidate split |
|---|---|---|
| `crates/consensus/src/mempool/queue.rs` | 1,665 | per-bucket / per-policy split |
| `crates/consensus/src/opcert.rs` | 856 | per-rule split (issue / verify / counter) |
| `crates/consensus/src/nonce.rs` | 832 | TPraos vs Praos vs evolution split |
| `crates/consensus/src/mempool/tx_state.rs` | 768 | per-state split |
| `crates/consensus/src/diffusion_pipelining.rs` | 747 | per-state-machine slice |
| `crates/consensus/src/chain_state.rs` | 654 | volatile vs immutable split |

R273b candidate: split `nonce.rs` (832 lines) along the upstream
`Cardano.Protocol.TPraos.Nonce` / `Ouroboros.Consensus.Protocol.Praos.Nonce`
boundary into `nonce/{tpraos,praos,evolution}.rs`.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R273
- R271s closure: `2026-05-07-round-271s-runtime-final-folds.md`
- Upstream Praos protocol:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/Praos.hs`
- Upstream Praos VRF:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/Praos/VRF.hs`
- Upstream ActiveSlotCoeff:
  `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/BaseTypes.hs`
