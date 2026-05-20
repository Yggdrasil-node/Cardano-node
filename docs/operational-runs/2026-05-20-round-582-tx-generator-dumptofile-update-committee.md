---
title: "Round 582 tx-generator DumpToFile UpdateCommittee GovAction"
parent: Reference
---

# Round 582 tx-generator DumpToFile UpdateCommittee GovAction

Date: 2026-05-20

## Scope

This round lifts the `UpdateCommittee` GovAction boundary in
`show_conway_gov_action`. The variant renders as upstream
`UpdateCommittee <StrictMaybe GovPurposeId> (fromList
[<KeyHashObj/ScriptHashObj>,...]) (fromList
[(<KeyHashObj/ScriptHashObj>,EpochNo <n>),...]) (<num> % <den>)`
matching stock-derived `Show (GovAction era)`.

After this round, `ParameterChange` is the only `GovAction` variant
remaining on `TxGenError` — pending the upstream
`ProtocolParameterUpdate` Show port (~30 optional `PParamUpdate`
fields).

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Procedures.hs:834-843`
  (`UpdateCommittee` 4-arg variant).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/BaseTypes.hs:293-298,545-558`
  (`BoundedRatio`, `UnitInterval` newtype-derived Show chain).
- `.reference-haskell-cardano-node/deps/cardano-base/cardano-slotting/src/Cardano/Slotting/Slot.hs:118-120`
  (`EpochNo` Show via Quiet).

## Changes

- Added `show_stake_credential` rendering yggdrasil
  `StakeCredential` as upstream `Credential r` (the phantom role
  tag — `Staking`, `ColdCommitteeRole`, etc. — does not affect
  Show output, so a single helper covers all role tags
  structurally).
- Added `show_unit_interval` rendering yggdrasil `UnitInterval` as
  upstream `(<num> % <den>)`. UnitInterval is a newtype over
  `BoundedRatio UnitInterval Word64`, which is a newtype over
  `Ratio Word64`. With `deriving newtype Show` on both newtypes,
  the chain delegates to `Show (Ratio Word64)` which emits `<num>
  % <den>` (wrapped in parens at showsPrec > 7 due to ratioPrec).
  The helper always wraps with parens so it's safe in
  constructor-argument position; record-field callers strip the
  outer parens if needed.
- Replaced the `UpdateCommittee` rejection in
  `show_conway_gov_action` with positive rendering:
  - prev_action_id via `show_strict_maybe_gov_purpose_id`
  - members_to_remove sorted by upstream `Ord StakeCredential` to
    match upstream `Set` iteration
  - members_to_add follows yggdrasil `BTreeMap` iteration matching
    upstream `Data.Map toAscList`
  - quorum via `show_unit_interval`
- Added 3 focused unit tests covering `show_unit_interval`,
  `show_stake_credential` KeyHash + ScriptHash variants, and
  UpdateCommittee both an empty/minimal form and a full form with
  SJust prev, one removal credential, one addition credential with
  EpochNo, and a non-trivial quorum.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile` (47 tests, +3
  from R581)
- `cargo test -p yggdrasil-tx-generator` (230 lib tests + 5
  CLI/golden, +3 from R581 baseline)

## Remaining

- Render the last complex `GovAction` variant `ParameterChange` —
  needs upstream `ProtocolParameterUpdate` Show coverage (~30
  optional `PParamUpdate` fields, each with its own per-type Show
  semantics).
- Close upstream `bootstrapWitKeyHash` byte-parity for
  multi-witness sets.
- Full Haskell `Show (ByteString)` mnemonic-escape coverage for
  byte parity.
- Capture upstream-binary comparison evidence once a runnable
  upstream binary environment is available.
