---
title: "Round 652 Conway GovAction scaffold + GOV tags 1/15 (A5 Phase-2.5)"
parent: Reference
---

# Round 652 Conway GovAction scaffold + GOV tags 1/15 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `GovAction` 7-variant scaffold (the Conway governance
action type) and wires Conway GOV tags 1 (`MalformedProposal`)
and 15 (`ZeroTreasuryWithdrawals`), both of which carry a
`GovAction era` payload. After R652, **17 of 19 Conway GOV
variants carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Procedures.hs:810-884`
  (`data GovAction era` 7-variant ADT — ParameterChange /
  HardForkInitiation / TreasuryWithdrawals / NoConfidence /
  UpdateCommittee / NewConstitution / InfoAction; CBOR `Sum`
  decoder tags 0-6).

## Changes

- Added `GovAction` 7-variant enum. The variant tag and
  constructor name are typed; the per-variant payloads
  (PParamsUpdate, Constitution, UnitInterval, treasury-withdrawal
  maps, nested `decodeNullStrictMaybe` GovPurposeIds) keep raw
  inner CBOR pending their typed decoder ports. `InfoAction`
  (tag 6) is fully typed — it has no payload.
- `GovAction::from_decoder` walks the outer CBOR array, reads the
  Word8 tag, and — for the payload-bearing variants — advances
  the decoder past each remaining element via the ledger
  decoder's recursive `skip()` then captures the payload bytes
  by byte range.
- Display: `InfoAction` renders bare; the other 6 variants emit
  `<Constructor> <raw-cbor N bytes>`.
- Refactored `ConwayGovPredFailure::MalformedProposal(Vec<u8>)`
  → `MalformedProposal(GovAction)` and
  `ZeroTreasuryWithdrawals(Vec<u8>)` →
  `ZeroTreasuryWithdrawals(GovAction)`. Both `from_cbor`
  branches decode the 2-element envelope `[tag, GovAction]`.

2 new focused unit tests:
- `_malformed_proposal_info_action_tag1` — GovAction InfoAction
  (the fully-typed no-payload variant) nested in MalformedProposal.
- `_zero_treasury_withdrawals_tag15` — GovAction
  TreasuryWithdrawals (raw payload) nested in
  ZeroTreasuryWithdrawals.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (318 lib + 4
  doctests + 1 main, +2 new tests vs R651 baseline of 316)

## Remaining (A5 Phase-2.5+)

- Conway GOV raw variants (2) — `ProposalProcedure era` (tags 8
  InvalidPrevGovActionId / 12 DisallowedProposalDuringBootstrap).
- GovAction per-variant typed payloads (PParamsUpdate,
  Constitution, UnitInterval, treasury-withdrawal maps).
- Conway UTXOW tag 10 (MissingRedeemers — PlutusPurpose AsItem).
- Conway UTXOS `CollectErrors`.
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
