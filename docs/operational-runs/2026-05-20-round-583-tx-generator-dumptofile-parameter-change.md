---
title: "Round 583 tx-generator DumpToFile ParameterChange GovAction"
parent: Reference
---

# Round 583 tx-generator DumpToFile ParameterChange GovAction

Date: 2026-05-20

## Scope

This round lifts the `ParameterChange` GovAction boundary in
`show_conway_gov_action` for the empty PParamsUpdate path. The
variant now renders as upstream `ParameterChange <StrictMaybe
GovPurposeId> (ConwayPParams {<30 fields all SNothing>})
<StrictMaybe ScriptHash>`. Non-empty `protocol_param_update` values
return a field-name-bearing TxGenError pending per-type Show ports
for the rich domain types each field wraps.

After this round, all 7 `GovAction` variants render for the
empty-PParamsUpdate path. This completes the GovAction variant set
structurally; the remaining work is per-type field Show coverage for
non-empty updates.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/PParams.hs:573-709`
  (`THKD`, `ConwayPParams` 30-field record).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Procedures.hs:811-818`
  (`ParameterChange` 3-arg variant).

## Changes

- Replaced the `ParameterChange` rejection in
  `show_conway_gov_action` with positive rendering.
- Added `show_conway_pparams_update` helper. Field order matches
  upstream `ConwayPParams`: `cppTxFeePerByte`, `cppTxFeeFixed`,
  `cppMaxBBSize`, `cppMaxTxSize`, `cppMaxBHSize`, `cppKeyDeposit`,
  `cppPoolDeposit`, `cppEMax`, `cppNOpt`, `cppA0`, `cppRho`,
  `cppTau`, `cppProtocolVersion` (HKDNoUpdate → `NoUpdate` not
  `SNothing`), `cppMinPoolCost`, `cppCoinsPerUTxOByte`,
  `cppCostModels`, `cppPrices`, `cppMaxTxExUnits`,
  `cppMaxBlockExUnits`, `cppMaxValSize`, `cppCollateralPercentage`,
  `cppMaxCollateralInputs`, `cppPoolVotingThresholds`,
  `cppDRepVotingThresholds`, `cppCommitteeMinSize`,
  `cppCommitteeMaxTermLength`, `cppGovActionLifetime`,
  `cppGovActionDeposit`, `cppDRepDeposit`, `cppDRepActivity`,
  `cppMinFeeRefScriptCostPerByte`.
- THKD transparent (its Show is `show . unTHKD`), so each field
  just renders the inner `StrictMaybe value`.
- Non-empty updates report all set fields by name (Conway names)
  including Shelley-era-only fields that Conway dropped (`d`,
  `extra_entropy`, `min_utxo_value`, `protocol_version`).
- `guardrails_script_hash` renders at showsPrec 11 (constructor
  argument position): `SNothing` or `(SJust (ScriptHash "..."))`.
- Converted the prior ParameterChange rejection test to an empty-
  envelope acceptance test; added a non-empty rejection test
  asserting the field name appears in the error message.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile` (49 tests, +2
  from R582)
- `cargo test -p yggdrasil-tx-generator` (231 lib tests + 5
  CLI/golden, +1 from R582 baseline)

## Remaining

- Port per-type Shows for the rich PParamsUpdate fields:
  - `CoinPerByte`, `CompactForm Coin` for the deposit / fee
    fields.
  - `EpochInterval` for `cppEMax`, `cppCommitteeMaxTermLength`,
    `cppGovActionLifetime`, `cppDRepActivity`.
  - `NonNegativeInterval` for `cppA0`,
    `cppMinFeeRefScriptCostPerByte`.
  - `Prices` for `cppPrices` (combines `prMem` + `prSteps`
    NonNegativeIntervals).
  - `OrdExUnits` for `cppMaxTxExUnits`, `cppMaxBlockExUnits`.
  - `PoolVotingThresholds` / `DRepVotingThresholds` records.
  - `CostModels` record with per-language cost-model maps.
- Close upstream `bootstrapWitKeyHash` byte-parity for
  multi-witness sets.
- Full Haskell `Show (ByteString)` mnemonic-escape coverage.
- Capture upstream-binary comparison evidence once a runnable
  upstream binary environment is available.
