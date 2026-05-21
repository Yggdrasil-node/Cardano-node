---
title: "Round 671 Typed GovAction NewConstitution (A5 Phase-2.5)"
parent: Reference
---

# Round 671 Typed GovAction NewConstitution (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `Constitution` type and types the
`GovAction::NewConstitution` variant (tag 5). After R671, 5 of
7 `GovAction` variants are fully typed.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Procedures.hs:844-848,886,912-958`
  (`NewConstitution (StrictMaybe (GovPurposeId
  'ConstitutionPurpose)) (Constitution era)`; `data
  Constitution era = Constitution { constitutionAnchor ::
  Anchor, constitutionGuardrailsScriptHash :: StrictMaybe
  ScriptHash }` — CBOR 2-element record `[Anchor,
  decodeNullStrictMaybe ScriptHash]`).

## Changes

- Added `Constitution { anchor: Anchor, guardrail: Option<[u8;
  28]> }` — decodes the 2-element record. Display matches
  upstream stock-derived record `Show`.
- Refactored `GovAction::NewConstitution(Vec<u8>)` → struct
  variant `{ prev: Option<GovActionId>, constitution:
  Constitution }`. `GovAction::from_decoder` special-cases tag
  5: decodes the 3-element envelope `[5, decodeNullStrictMaybe
  GovPurposeId, Constitution]`.
- Display: `NewConstitution (<StrictMaybe GovPurposeId>)
  (<Constitution>)`.

1 new focused unit test:
- `conway_gov_pred_failure_malformed_proposal_new_constitution`
  — a `MalformedProposal` carrying a `NewConstitution`
  GovAction, asserting the nested `Constitution` record render.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (339 lib + 4
  doctests + 1 main, +1 new test vs R670 baseline of 338)

## Remaining (A5 Phase-2.5+)

- `GovAction` raw variants: tag 0 ParameterChange
  (PParamsUpdate), tag 4 UpdateCommittee.
- Deepest leaf payloads: `PParamsUpdate`, `ContextError`.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
