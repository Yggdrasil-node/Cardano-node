---
title: "Round 581 tx-generator DumpToFile TreasuryWithdrawals GovAction"
parent: Reference
---

# Round 581 tx-generator DumpToFile TreasuryWithdrawals GovAction

Date: 2026-05-20

## Scope

This round lifts the `TreasuryWithdrawals` GovAction boundary in
`show_conway_gov_action`. The variant renders as upstream
`TreasuryWithdrawals (fromList [(AccountAddress {...}, Coin <n>),
...]) <StrictMaybe ScriptHash>` matching stock-derived
`Show (GovAction era)`.

`ParameterChange` and `UpdateCommittee` remain on explicit
`TxGenError` pending their internal-type Show ports
(`ProtocolParameterUpdate` and `UnitInterval` respectively).

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Procedures.hs:825-829`
  (`TreasuryWithdrawals !(Map AccountAddress Coin) !(StrictMaybe
  ScriptHash)`).

## Changes

- Added `show_account_address_from_record` typed helper that
  renders upstream `AccountAddress {aaNetworkId, aaId =
  KeyHashObj/ScriptHashObj}` from a yggdrasil `RewardAccount`
  directly, without the byte-decoding round-trip needed by the
  R580 `show_account_address` helper.
- Replaced the TreasuryWithdrawals rejection in
  `show_conway_gov_action` with positive rendering: builds the
  `Map AccountAddress Coin` body from yggdrasil's
  `BTreeMap<RewardAccount, u64>`, then assembles the full
  `TreasuryWithdrawals (fromList [...]) <guardrails>` form. The
  guardrails_script_hash field renders at showsPrec 11 (constructor
  argument position): `SNothing` or `(SJust (ScriptHash "..."))`.
- Added `dumptofile_show_conway_gov_action_treasury_withdrawals`
  unit test covering the empty-withdrawals + SNothing minimal form
  and a single-key-hash-withdrawal + SJust-guardrails full form.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile` (44 tests, +1
  from R580)
- `cargo test -p yggdrasil-tx-generator` (227 lib tests + 5
  CLI/golden, +1 from R580 baseline)

## Remaining

- `ParameterChange` GovAction — needs `ProtocolParameterUpdate`
  Show (~30 optional `PParamUpdate` fields, all of which need
  stock-Show field-by-field rendering).
- `UpdateCommittee` GovAction — needs `UnitInterval` Show (rational
  number) plus the `Set (Credential ColdCommitteeRole)` and
  `Map (Credential ColdCommitteeRole) EpochNo` Shows.
- Close upstream `bootstrapWitKeyHash` byte-parity for
  multi-witness sets (Byron AddressInfo Blake2b-224).
- Full Haskell `Show (ByteString)` mnemonic-escape coverage for
  byte parity.
- Capture upstream-binary comparison evidence once a runnable
  upstream binary environment is available.
