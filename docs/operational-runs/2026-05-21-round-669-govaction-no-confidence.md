---
title: "Round 669 Typed GovAction NoConfidence (A5 Phase-2.5)"
parent: Reference
---

# Round 669 Typed GovAction NoConfidence (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types the `GovAction::NoConfidence` variant (tag 3). After R669,
3 of 7 `GovAction` variants (`InfoAction`, `HardForkInitiation`,
`NoConfidence`) are fully typed.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Procedures.hs:830-833,884`
  (`NoConfidence (StrictMaybe (GovPurposeId 'CommitteePurpose))`;
  decoder `3 -> SumD NoConfidence <! D (decodeNullStrictMaybe
  decCBOR)`).

## Changes

- Refactored `GovAction::NoConfidence(Vec<u8>)` → struct variant
  `{ prev: Option<GovActionId> }`.
- `GovAction::from_decoder` special-cases tag 3: decodes the
  2-element envelope `[3, decodeNullStrictMaybe GovPurposeId]`.
- Display: `NoConfidence (<StrictMaybe GovPurposeId>)`.

1 new focused unit test:
- `conway_gov_pred_failure_malformed_proposal_no_confidence` — a
  `MalformedProposal` carrying a `NoConfidence` GovAction with a
  `SJust GovActionId` prev.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (337 lib + 4
  doctests + 1 main, +1 new test vs R668 baseline of 336)

## Remaining (A5 Phase-2.5+)

- `GovAction` raw variants: tag 0 ParameterChange
  (PParamsUpdate), tag 2 TreasuryWithdrawals (Map +
  ScriptHash), tag 4 UpdateCommittee, tag 5 NewConstitution
  (Constitution).
- Deepest leaf payloads: `PParamsUpdate`, `Constitution`,
  `ContextError`.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
