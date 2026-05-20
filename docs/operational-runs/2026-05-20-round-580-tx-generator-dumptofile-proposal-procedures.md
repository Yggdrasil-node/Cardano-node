---
title: "Round 580 tx-generator DumpToFile Conway ProposalProcedures"
parent: Reference
---

# Round 580 tx-generator DumpToFile Conway ProposalProcedures

Date: 2026-05-20

## Scope

This round lifts the Conway `ctbrProposalProcedures` boundary for
the 4 simple `GovAction` variants. Previously, any Conway
transaction with non-empty `proposal_procedures` failed
`SubmitMode::DumpToFile` with `does not yet support non-empty
ctbrProposalProcedures`. After this round, proposal procedures
render as upstream stock-derived `Show (OSet (ProposalProcedure
era))` over the OSet shell + `ProposalProcedure` record + 4 simple
`GovAction` variants.

Complex `GovAction` variants (`ParameterChange`,
`TreasuryWithdrawals`, `UpdateCommittee`) remain on explicit
`TxGenError` pending their internal-type Show ports.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Procedures.hs:498-504,810-861,912-943`
  (`ProposalProcedure`, `GovAction`, `Constitution`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/BaseTypes.hs:207`
  (`ProtVer`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Address.hs:183-191`
  (`AccountAddress`, `AccountId`).

## Changes

- Replaced the `ensure_empty_or_absent` rejection in
  `show_conway_tx_for_dump` with positive rendering via new
  `show_conway_proposal_procedures`. The OSet shell emits
  `OSet {osSSeq = StrictSeq {fromStrict = fromList [...]}, osSet =
  fromList [...]}` matching upstream stock-derived `Show OSet`.
- Added `show_conway_proposal_procedure` rendering the 4-field
  record (`pProcDeposit`, `pProcReturnAddr`, `pProcGovAction`,
  `pProcAnchor`).
- Added `show_account_address` decoding the yggdrasil 29-byte
  reward-account bytes through `RewardAccount::from_bytes` into
  upstream `AccountAddress {aaNetworkId :: Network, aaId ::
  AccountId}` form. `AccountId`'s newtype-derived Show delegates to
  the inner `Credential Staking` Show (`KeyHashObj (KeyHash {...})`
  or `ScriptHashObj (ScriptHash "...")`).
- Added `show_conway_gov_action` covering 4 simple variants:
  - `InfoAction` (no field)
  - `NoConfidence (StrictMaybe GovPurposeId)`
  - `HardForkInitiation (StrictMaybe GovPurposeId) (ProtVer {pvMajor
    = Version <n>, pvMinor = <n>})`
  - `NewConstitution (StrictMaybe GovPurposeId) (Constitution
    {constitutionAnchor = ..., constitutionGuardrailsScriptHash =
    SNothing | SJust (ScriptHash "...")})`
  The 3 complex variants return informative `TxGenError` boundaries
  with the missing-Show name (ProtocolParameterUpdate /
  AccountAddress-map / UnitInterval).
- Added `show_strict_maybe_gov_purpose_id` rendering
  `StrictMaybe (GovPurposeId p)`. The phantom-tagged
  `GovPurposeId` newtype uses `deriving newtype Show`, so it
  delegates directly to `GovActionId`'s record Show — output is
  `SNothing` or `(SJust GovActionId {gaidTxId = TxId ..., ...})`.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile` (43 tests, +6
  from R579)
- `cargo test -p yggdrasil-tx-generator` (226 lib tests + 5
  CLI/golden, +6 from R579 baseline)

## Remaining

- Render the 3 complex `GovAction` variants:
  - `ParameterChange` — needs `ProtocolParameterUpdate` Show
    (~30 optional `PParamUpdate` fields).
  - `TreasuryWithdrawals` — needs the `Map AccountAddress Coin`
    Show.
  - `UpdateCommittee` — needs `UnitInterval` Show plus the
    members-to-add `Map (Credential ColdCommitteeRole) EpochNo`
    Show.
- Close upstream `bootstrapWitKeyHash` byte-parity for multi-witness
  sets (Byron AddressInfo Blake2b-224).
- Full Haskell `Show (ByteString)` mnemonic-escape coverage for
  `\NUL`/`\SOH`/.../`\DEL` byte parity.
- Capture upstream-binary comparison evidence once a runnable
  upstream binary environment is available.
