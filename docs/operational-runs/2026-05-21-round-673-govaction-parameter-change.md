---
title: "Round 673 Typed GovAction ParameterChange + PParamsUpdate scaffold (A5 Phase-2.5)"
parent: Reference
---

# Round 673 Typed GovAction ParameterChange + PParamsUpdate scaffold (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `PParamsUpdate` scaffold and types the
`GovAction::ParameterChange` variant (tag 0) — the last raw
GovAction variant. **All 7 GovAction variants now carry typed
payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Procedures.hs:811-818,874-878`
  (`ParameterChange (StrictMaybe (GovPurposeId
  'PParamUpdatePurpose)) (PParamsUpdate era) (StrictMaybe
  ScriptHash)`; decoder `0 -> SumD ParameterChange <! D
  (decodeNullStrictMaybe decCBOR) <! From <! D
  (decodeNullStrictMaybe decCBOR)`).

## Changes

- Added `PParamsUpdate { updates: Vec<(u64, Vec<u8>)> }` — a
  scaffold decoding the CBOR parameter-update map; each value is
  captured raw by byte range, surfacing the set of updated
  parameter ids and the count. Display: `PParamsUpdate (fromList
  [(<id>,<raw-cbor N bytes>),...])`.
- Refactored `GovAction::ParameterChange(Vec<u8>)` → struct
  variant `{ prev: Option<GovActionId>, pparams_update:
  PParamsUpdate, guardrail: Option<[u8; 28]> }`.
- `GovAction::from_decoder` special-cases tag 0: decodes the
  4-element envelope `[0, decodeNullStrictMaybe GovPurposeId,
  PParamsUpdate, decodeNullStrictMaybe ScriptHash]`. With every
  tag now typed, the former raw-capture fallthrough was removed.

1 new focused unit test:
- `conway_gov_pred_failure_malformed_proposal_parameter_change`
  — a `MalformedProposal` carrying a `ParameterChange` GovAction
  with a 2-parameter `PParamsUpdate`.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (341 lib + 4
  doctests + 1 main, +1 new test vs R672 baseline of 340)

## Remaining (A5 Phase-2.5+)

- `PParamsUpdate` per-parameter typed values (~30 protocol
  parameters).
- Deepest leaf payloads: `ContextError`, `CollectError`
  NoRedeemer.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
