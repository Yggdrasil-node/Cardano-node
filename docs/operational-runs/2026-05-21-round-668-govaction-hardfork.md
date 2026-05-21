---
title: "Round 668 Typed GovAction HardForkInitiation (A5 Phase-2.5)"
parent: Reference
---

# Round 668 Typed GovAction HardForkInitiation (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types the `GovAction::HardForkInitiation` variant (tag 1) — the
simplest payload-bearing governance action. After R668, 2 of 7
`GovAction` variants (`InfoAction`, `HardForkInitiation`) are
fully typed.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Procedures.hs:819-824,879-882`
  (`HardForkInitiation (StrictMaybe (GovPurposeId
  'HardForkPurpose)) ProtVer`; decoder `SumD HardForkInitiation
  <! D (decodeNullStrictMaybe decCBOR) <! D (decodeProtVer)`).

## Changes

- Refactored `GovAction::HardForkInitiation(Vec<u8>)` → struct
  variant `{ prev: Option<GovActionId>, protver: ProtVer }`.
- `GovAction::from_decoder` special-cases tag 1 (after the
  tag-6 `InfoAction` case): decodes the 3-element envelope `[1,
  decodeNullStrictMaybe GovPurposeId, ProtVer]` via the R667
  `decode_null_strict_maybe` helper and `ProtVer::from_decoder`.
- Display: `HardForkInitiation (<StrictMaybe GovPurposeId>)
  (<ProtVer>)`.

1 new focused unit test:
- `conway_gov_pred_failure_malformed_proposal_hard_fork` — a
  `MalformedProposal` carrying a `HardForkInitiation` GovAction
  (SNothing prev + ProtVer 10.0), asserting the full nested
  Display.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (336 lib + 4
  doctests + 1 main, +1 new test vs R667 baseline of 335)

## Remaining (A5 Phase-2.5+)

- `GovAction` raw variants: tag 0 ParameterChange
  (PParamsUpdate), tag 2 TreasuryWithdrawals (Map +
  ScriptHash), tag 3 NoConfidence (StrictMaybe GovPurposeId),
  tag 4 UpdateCommittee, tag 5 NewConstitution (Constitution).
- Deepest leaf payloads: `PParamsUpdate`, `Constitution`,
  `ContextError`.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
