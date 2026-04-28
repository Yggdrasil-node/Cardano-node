## Round 161 — Conway PParams shape (31-element list) + PV→era regression tests

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Close Round 160's open follow-ups: complete per-era PP dispatch by
adding the Conway shape (mainnet's current era), and pin the
PV-major → era_index table with unit tests so future regressions
fail CI cleanly instead of silently shifting era reporting.

### Implementation

`crates/network/src/protocols/local_state_query_upstream.rs`:

- New `encode_conway_pparams_for_lsq(params)` emits the 31-element
  CBOR list per upstream `Cardano.Ledger.Conway.PParams.encCBOR`:
  - 22 Babbage fields (1-22 in order: minfeeA/B, maxBBSize/TxSize/BHSize,
    keyDeposit, poolDeposit, eMax, nOpt, a0, rho, tau, protocolVersion,
    minPoolCost, coinsPerUtxoByte, costModels, prices, maxTxExUnits,
    maxBlockExUnits, maxValSize, collateralPercentage, maxCollateralInputs)
  - 9 governance fields:
    23. poolVotingThresholds (5-element UnitInterval list)
    24. drepVotingThresholds (10-element UnitInterval list)
    25. minCommitteeSize (u64)
    26. committeeTermLimit (u64 epoch)
    27. govActionLifetime (u64 epoch)
    28. govActionDeposit (u64 lovelace)
    29. drepDeposit (u64 lovelace)
    30. drepActivity (u64 epoch)
    31. minFeeRefScriptCostPerByte (UnitInterval)
  - Defaults match Conway-genesis mainnet values when missing
    fields aren't carried by the snapshot.

`node/src/local_server.rs`:

- Dispatcher's `GetCurrentPParams` branch now wires
  era_index=6 → `encode_conway_pparams_for_lsq`.
- Per-era PP table is now complete: 1..=3 Shelley (17), 4 Alonzo
  (24), 5 Babbage (22), 6 Conway (31).

### Regression tests

`node/src/local_server.rs`:

- `effective_era_index_pv_table_matches_upstream` — pins the PV
  major → era_index mapping for PV 1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
  100 against upstream's `*Transition` ProtVer table.
- `effective_era_index_falls_back_to_params_pv_when_no_block` —
  pins the `protocol_params.protocol_version` fallback when no
  block PV has been recorded yet.
- `effective_era_index_never_demotes_below_wire_era` — pins the
  "never demote" rule when wire era_tag is higher than PV-derived
  era.

`crates/network/src/protocols/local_state_query_upstream.rs`:

- `babbage_pparams_emit_22_element_list` — pins `0x96` = array(22)
  prefix.
- `conway_pparams_emit_31_element_list` — pins `0x98 0x1f` =
  array(31) prefix.

### Test results

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4706  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4701 (Round 160) → 4706.

### Cumulative cardano-cli `query protocol-parameters` parity

Yggdrasil now serves the right PP shape for any snapshot reporting
any era from Shelley through Conway:

| Era / era_index | Field count | Key fields |
|---|---|---|
| Shelley (1) / Allegra (2) / Mary (3) | 17 | minUTxOValue |
| Alonzo (4) | 24 | + costModels, prices, exUnits, maxValSize, collateral |
| Babbage (5) | 22 | drops d/extraEntropy, renames coinsPerUtxoByte |
| Conway (6) | 31 | + DRep/pool voting thresholds, committee, gov actions, refScriptFee |

### Open follow-ups

1. **Babbage+ TxOut encoding** — current
   `encode_txout_era_specific` handles Shelley/Mary/Alonzo/Babbage
   internal MultiEraTxOut variants, but Babbage TxOut adds
   optional `datum_inline` and `script_ref` fields beyond
   Alonzo's `datum_hash`.  Required for `query utxo --whole-utxo`
   to render correctly when synced past Alonzo.
2. **Era summaries past Shelley** — `encode_interpreter_for_network`
   returns 2-era preprod/mainnet summaries; chains in Babbage+
   will eventually need explicit Allegra/Mary/Alonzo/Babbage
   entries for accurate slot↔epoch math past the synthetic
   far-future end.
3. **`query stake-address-info`** — needs Bech32 stake-address
   parsing + `GetFilteredDelegationsAndRewardAccounts` (tag 10)
   dispatcher.  Currently era-blocked client-side until Babbage+
   snapshot reported.

### References

- `Cardano.Ledger.Conway.PParams.encCBOR` — 31-element shape.
- Previous round: `docs/operational-runs/2026-04-28-round-160-babbage-pparams-pv-era.md`.
- Code: `crates/network/src/protocols/local_state_query_upstream.rs`,
  `node/src/local_server.rs`.
