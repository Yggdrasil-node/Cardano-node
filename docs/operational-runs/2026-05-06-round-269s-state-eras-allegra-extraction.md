## Round 269s — `state.rs` per-era split: nineteenth slice (Allegra apply)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 nineteenth slice — strict 1:1 with upstream Haskell, per-era impl carve)

### Slice scope

Extracted 187 source lines from `state.rs::impl LedgerState::apply_allegra_block`
into `crates/ledger/src/state/eras/allegra.rs` (213 lines). Wired
`pub(super) mod allegra;` into the eras `mod.rs` (alongside R269q's
byron and R269r's shelley).

Allegra's apply path differs from Shelley in two ways:
- Uses `multi_era_utxo` directly (no `shelley_utxo` mirror commit).
- Uses `staged.apply_allegra_tx_withdrawals` (multi-era apply path with
  Allegra's validity-interval semantics) rather than Shelley's
  `apply_tx_with_withdrawals`.

Cross-module references (validators in `state::phase1_validation`, free
fns and methods in `state.rs`) all resolve through the visibility model
documented in R269r — descendants see ancestor's private items
automatically; only the apply method itself needs `pub(in crate::state)`.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 8,188 | 8,000 | −188 |
| `crates/ledger/src/state/eras/mod.rs` | 19 | 20 | +1 |
| `crates/ledger/src/state/eras/allegra.rs` | (new) | 213 | +213 |

Dropped now-unused `use crate::eras::allegra::AllegraTxBody;` from
state.rs (line 1).

### Cumulative R269 progress

| Slice | File created | Lines moved | state.rs running size |
|---|---|---|---|
| R269 – R269p | (16 sibling submodules + cbor codec) | 4,295 | 8,409 |
| R269q (Byron apply) | `state/eras/byron.rs` (60) | 32 | 8,377 |
| R269r (Shelley apply) | `state/eras/shelley.rs` (224) | 189 | 8,188 |
| **R269s (Allegra apply)** | **`state/eras/allegra.rs` (213)** | **188** | **8,000** |

Net reduction: **12,704 → 8,000 lines (−4,704, ~37 %)** with 17 sibling
files plus a `state/eras/` subdirectory containing 3 era files.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R269s
- Upstream Allegra rules:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/allegra/impl/src/Cardano/Ledger/Allegra/Rules/{Bbody,Ledger,Utxow,Utxo}.hs`
