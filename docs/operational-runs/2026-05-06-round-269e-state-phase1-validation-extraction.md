## Round 269e — `state.rs` per-rule split: fifth slice (Phase-1 validation helpers)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 fifth slice — strict 1:1 with upstream Haskell)

### Context

Continuing R269's per-rule carve of `crates/ledger/src/state.rs`. After
`state/{mir,ratify,enact,deposit_pot}.rs` shipped (R269 a–d), this slice
extracts the **Phase-1 transaction validation helpers** — 25 private
functions that gate transactions on protocol parameters before phase-2
script execution.

### Slice scope

Extracted ~792 source lines from `state.rs` lines 10712–11503 into
`crates/ledger/src/state/phase1_validation.rs`. The 25 helpers each
mirror a specific predicate from upstream's UTXO / UTXOW rule families:

**Tx-level fee + size + ExUnits checks**
- `validate_pre_alonzo_tx` — pre-Alonzo tx-size, fee, min-UTxO,
  multi-asset value-size + boot-addr attribute checks. Mirrors
  `Cardano.Ledger.Mary.Rules.Utxo`.
- `validate_alonzo_plus_tx` — Alonzo+ tx-size, Conway-tier fee +
  per-tx ExUnits + collateral validation (mandatory when redeemers
  present). Mirrors `Cardano.Ledger.Alonzo.Rules.Utxo::feesOK`.
- `validate_block_ex_units` — total-block ExUnits cap. Mirrors
  Alonzo BBODY.
- `sum_redeemer_ex_units`, `validate_per_redeemer_ex_units_*`,
  `sum_redeemer_ex_units_from_bytes` — per-redeemer ExUnits decoding
  and summation helpers.

**Witness-coverage checks**
- `validate_witnesses_if_present` (raw bytes) and
  `validate_witnesses_typed` — VKey + bootstrap witness coverage and
  signature verification. Mirrors
  `Cardano.Ledger.Core::keyHashWitnessesTxWits`.
- `validate_native_scripts_if_present` — native-script witness
  evaluation against required script hashes.
- `validate_required_script_witnesses` — every required script hash
  is satisfied by either a native or Plutus witness.
- `provided_script_hashes_from_witnesses`,
  `collect_reference_script_hashes` — script-hash provenance helpers.
- `validate_no_extraneous_script_witnesses{,_typed}` — every provided
  script must be required (sNeeded \\ sRefs ⊇ sReceived). Mirrors
  Alonzo UTXOW `extraneousScriptWitnessesUTXOW`.

**Auxiliary data hash + metadatum validation**
- `validate_auxiliary_data` — declared hash matches content hash
  (Blake2b-256), with metadata-size sub-check on PV > (2,0).
- `validate_auxiliary_data_metadata_sizes`, `validate_metadata_map`,
  `validate_metadatum` — recursive ≤64-byte byte/text checks on
  transaction-metadatum CBOR. Mirrors
  `Cardano.Ledger.Metadata::validMetadatum`.

**Network ID checks**
- `shelley_address_network_id` — header-byte → network nibble
  decoder, returning `None` for Byron/reserved types.
- `validate_output_network_ids`,
  `validate_withdrawal_network_ids`,
  `validate_tx_body_network_id` — Shelley `WrongNetwork{,Withdrawal}`
  and Alonzo `WrongNetworkInTxBody`.

**Misc helpers**
- `accumulate_multi_asset` — pure asset-merge accumulator.
- `relay_access_points_from_relays` — pool-relay → access-point
  conversion.

All 25 functions stay private to the `state` module — promoted from
`fn` to `pub(super) fn` so the new sibling submodule can be used from
`state.rs` while the symbols remain unreachable to other crates.

### Wiring

`state.rs` keeps a `pub(super) mod phase1_validation;` declaration
plus a glob `use phase1_validation::*;` at the top. The glob keeps
all ~120 internal call sites unqualified — no per-call-site edit
required.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `state/phase1_validation.rs::validate_pre_alonzo_tx` | `Cardano.Ledger.Shelley.Rules.Utxo` (ShelleyUTXO subset) |
| `state/phase1_validation.rs::validate_alonzo_plus_tx` | `Cardano.Ledger.Alonzo.Rules.Utxo::feesOK` |
| `state/phase1_validation.rs::validate_block_ex_units` | Alonzo BBODY ExUnit cap |
| `state/phase1_validation.rs::validate_witnesses_*` | `Cardano.Ledger.Shelley.Rules.Utxow::keyHashWitnessesTxWits` + `verifyTxWitnesses` |
| `state/phase1_validation.rs::validate_required_script_witnesses` | `Cardano.Ledger.Alonzo.Rules.Utxow` script-witness-coverage check |
| `state/phase1_validation.rs::validate_no_extraneous_script_witnesses{,_typed}` | `Cardano.Ledger.Alonzo.Rules.Utxow::extraneousScriptWitnessesUTXOW` |
| `state/phase1_validation.rs::validate_auxiliary_data*` | `Cardano.Ledger.Shelley.Rules.Utxow::validateMetadata` + `Cardano.Ledger.Metadata::validMetadatum` |
| `state/phase1_validation.rs::shelley_address_network_id` + `validate_*_network_id*` | `Cardano.Ledger.Shelley.Rules.Utxo::WrongNetwork{,Withdrawal}` + `Cardano.Ledger.Alonzo.Rules.Utxo::WrongNetworkInTxBody` |

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 11,506 | 10,714 | −792 |
| `crates/ledger/src/state/phase1_validation.rs` | (new) | 817 | +817 |

The `+25` net (817 − 792) is the new file's module-level docstring +
imports — actual code body is byte-identical to the original section.

State.rs imports tightened: `Relay`, `Ipv4Addr`, `Ipv6Addr` were only
referenced by `relay_access_points_from_relays` and are now reachable
only from the new submodule, so the parent `state.rs` drops them from
its `use` lists.

### Cumulative R269 progress

| Slice | File created | Lines moved | state.rs running size |
|---|---|---|---|
| R269  (MIR)         | `state/mir.rs` (123)         | 110   | 12,596 |
| R269b (Ratify)      | `state/ratify.rs` (675)      | 657   | 11,939 |
| R269c (Enact)       | `state/enact.rs` (362)       | 343   | 11,602 |
| R269d (DepositPot)  | `state/deposit_pot.rs` (124) | 106   | 11,496 |
| **R269e (Phase-1)** | **`state/phase1_validation.rs` (817)** | **792** | **10,714** |

Net `state.rs` reduction so far: **12,704 → 10,714 lines (−1,990)** with
five sibling files. State.rs is now back below 11k lines for the first
time since its R256 Phase H consolidation point.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 851 passed, 0 failed (unchanged from R269d)
```

Pure code-move refactor — no test changes. 25 private validation
helpers retained behaviour; existing tests in
`state/tests.rs::validate_alonzo_plus_tx_*` and the
`shelley_address_network_id_extracts_correctly` assertions still hit
the same code via `super::*` re-exports.

### Cumulative parity arc

| Round | Status | Effect |
|---|---|---|
| R266 / R266b / R266c | shipped | Gap BP narrowed to deep ScriptContext field encoding (operator-time-blocked) |
| R269 a–d | shipped | `state/{mir,ratify,enact,deposit_pot}.rs` extracted (1,216 lines moved) |
| **R269e** | **this round** | `state/phase1_validation.rs` extracted: 25 phase-1 UTXO/UTXOW predicate helpers (792 lines). State.rs cumulative reduction 1,990 lines (12,704 → 10,714). |

### Next R269 slices (queued)

1. **`state/ppup.rs`** — PPUP helpers section (`state.rs` lines
   45–~2140, ~2,098 lines). Mirrors `Cardano.Ledger.Shelley.Rules.Ppup`.
   Largest remaining slice; may benefit from sub-splitting into
   `ppup/{state,validate,apply}.rs`.
2. **`state/treasury.rs`** — `AccountingState` treasury/reserves
   tracking (small, ~30 lines if isolated).
3. **`state/chain_dep.rs`** — `ChainDepStateContext` + impls (the
   sidecar nonce/OCert mirror, ~50 lines).
4. Per-type files for `LedgerState`, `LedgerStateSnapshot`,
   `LedgerStateCheckpoint`, `PoolState`, etc. — the remaining
   structural bulk.

### References

- R269 a–d closures: `2026-05-06-round-269{,b,c,d}-state-*.md`
- Plan: `~/.claude/plans/dapper-giggling-haven.md`
- Upstream UTXO rule:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs`
- Upstream Alonzo UTXO + UTXOW (feesOK, witness coverage):
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/{Utxo,Utxow}.hs`
- Upstream metadatum validation:
  `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Metadata.hs`
