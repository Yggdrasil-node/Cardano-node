## Round 269t — `state.rs` per-era split: twentieth slice (Mary apply)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 twentieth slice — strict 1:1 with upstream Haskell, per-era impl carve)

### Slice scope

Extracted 181 source lines from `state.rs::impl LedgerState::apply_mary_block`
into `crates/ledger/src/state/eras/mary.rs` (208 lines). Wired
`pub(super) mod mary;` into the eras `mod.rs`.

Mary's apply path is structurally identical to Allegra except for two
era-specific differences:

- `MaryTxBody` carries `Value` outputs (multi-asset quantities) rather
  than Allegra's `Coin`-only `ShelleyTxOut`-shaped outputs.
- `staged.apply_mary_tx_withdrawals` (multi-asset transition) replaces
  Allegra's `apply_allegra_tx_withdrawals`.
- `MultiEraTxOut::Mary` wrapping replaces Allegra's `MultiEraTxOut::Shelley`
  for phase-1 fee/output validation.

No new helper-fn dependencies — Mary uses the same phase1_validation
and witness validators as Allegra.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 8,000 | 7,818 | −182 |
| `crates/ledger/src/state/eras/mod.rs` | 20 | 21 | +1 |
| `crates/ledger/src/state/eras/mary.rs` | (new) | 208 | +208 |

No state.rs imports needed cleanup — Mary types are referenced via
full path `crate::eras::mary::*` throughout, no top-level `use` line.

### Cumulative R269 progress

| Slice | File created | Lines moved | state.rs running size |
|---|---|---|---|
| R269 – R269p | (16 sibling submodules + cbor codec) | 4,295 | 8,409 |
| R269q (Byron apply) | `state/eras/byron.rs` (60) | 32 | 8,377 |
| R269r (Shelley apply) | `state/eras/shelley.rs` (224) | 189 | 8,188 |
| R269s (Allegra apply) | `state/eras/allegra.rs` (213) | 188 | 8,000 |
| **R269t (Mary apply)** | **`state/eras/mary.rs` (208)** | **182** | **7,818** |

Net reduction: **12,704 → 7,818 lines (−4,886, ~38 %)** with 17 sibling
files plus `state/eras/` containing 4 era files.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

### Stop point — Alonzo (R269u) is materially different

Alonzo apply (~423 lines) introduces the Plutus phase-2 evaluator:

- `evaluator: Option<&dyn crate::plutus_validation::PlutusEvaluator>` parameter.
- Block-level ExUnits limit via `validate_block_ex_units`.
- Script-data-hash validation via `crate::plutus_validation::validate_script_data_hash`.
- Per-redeemer ExUnits via `sum_redeemer_ex_units_from_bytes` +
  `validate_per_redeemer_ex_units_from_bytes`.
- Alonzo+ phase-1 via `validate_alonzo_plus_tx`.
- TxBody network-id validation via `validate_tx_body_network_id`.

R269u will pull in these helpers (same descendants-see-ancestors
visibility model — no promotions needed) and may introduce a few
imports from `phase1_validation` not used by Mary. Bounded scope but
~2.3× the size of Mary.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R269t
- Upstream Mary rules:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/mary/impl/src/Cardano/Ledger/Mary/Rules/{Bbody,Ledger,Utxow,Utxo}.hs`
