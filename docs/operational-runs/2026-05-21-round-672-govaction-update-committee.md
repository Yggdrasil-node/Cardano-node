---
title: "Round 672 Typed GovAction UpdateCommittee (A5 Phase-2.5)"
parent: Reference
---

# Round 672 Typed GovAction UpdateCommittee (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types the `GovAction::UpdateCommittee` variant (tag 4). After
R672, 6 of 7 `GovAction` variants are fully typed — only
`ParameterChange` remains raw.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Procedures.hs:834-843,885`
  (`UpdateCommittee (StrictMaybe (GovPurposeId
  'CommitteePurpose)) (Set (Credential ColdCommitteeRole)) (Map
  (Credential ColdCommitteeRole) EpochNo) UnitInterval`;
  decoder `4 -> SumD UpdateCommittee <! D
  (decodeNullStrictMaybe decCBOR) <! From <! From <! From`).

## Changes

- Refactored `GovAction::UpdateCommittee(Vec<u8>)` → struct
  variant `{ prev: Option<GovActionId>, remove: Vec<Credential>,
  add: Vec<(Credential, u64)>, threshold: UnitInterval }`.
- `GovAction::from_decoder` special-cases tag 4: decodes the
  5-element envelope `[4, decodeNullStrictMaybe GovPurposeId,
  Set Credential, Map Credential EpochNo, UnitInterval]` — the
  remove-set is tag-258-tolerant, the add-map is a plain CBOR
  map.
- Display: `UpdateCommittee (<StrictMaybe GovPurposeId>)
  (fromList [<remove>...]) (fromList [(<cred>,EpochNo <n>)...])
  (<UnitInterval>)`.

1 new focused unit test:
- `conway_gov_pred_failure_malformed_proposal_update_committee`
  — a `MalformedProposal` carrying an `UpdateCommittee`
  GovAction with a one-element remove-set, a one-entry add-map,
  and a 2/3 threshold.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (340 lib + 4
  doctests + 1 main, +1 new test vs R671 baseline of 339)

## Remaining (A5 Phase-2.5+)

- `GovAction::ParameterChange` (`PParamsUpdate` — the ~30-field
  protocol-parameter update record).
- Deepest leaf payloads: `PParamsUpdate`, `ContextError`.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
