---
title: "Round 651 Typed Conway GOV ProposalCantFollow variant (A5 Phase-2.5)"
parent: Reference
---

# Round 651 Typed Conway GOV ProposalCantFollow variant (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway GOV tag 10 (`ProposalCantFollow`) by adding the
`StrictMaybeGovPurposeId` type. After R651, **15 of 19 Conway
GOV variants carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Gov.hs:195-199,247,281-282`
  (`ProposalCantFollow (StrictMaybe (GovPurposeId
  'HardForkPurpose)) (Mismatch RelGT ProtVer)`; encoder `Sum
  ProposalCantFollow 10 !> To prevgaid !> ToGroup mm`).
- `newtype GovPurposeId (p :: GovActionPurpose) = GovPurposeId
  {unGovPurposeId :: GovActionId}` — a thin newtype over
  `GovActionId`.

## Changes

- Added `StrictMaybeGovPurposeId(Option<GovActionId>)` — decodes
  a `StrictMaybe (GovPurposeId p)` from a CBOR list (0-element =
  SNothing, 1-element = SJust GovActionId). Display: `SNothing`
  / `SJust (GovPurposeId {unGovPurposeId = <GovActionId>})`.
- Refactored `ConwayGovPredFailure::ProposalCantFollow(Vec<u8>)`
  → struct variant `{ prev: StrictMaybeGovPurposeId, mismatch:
  Mismatch<ProtVer> }`. `from_cbor` decodes the 4-element
  envelope `[10, StrictMaybe GovPurposeId, supplied ProtVer,
  expected ProtVer]` (the Mismatch is ToGroup-flattened, each
  ProtVer a 2-element array). Display:
  `ProposalCantFollow (<prev>) (<Mismatch ProtVer>)`.

1 new focused unit test:
- `_proposal_cant_follow_tag10` — SNothing prev + a ProtVer
  9→10 Mismatch, asserting the full nested Display.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (316 lib + 4
  doctests + 1 main, +1 new test vs R650 baseline of 315)

## Remaining (A5 Phase-2.5+)

- Conway GOV raw variants (4) — typed decoders for `GovAction
  era` (tags 1 MalformedProposal / 15 ZeroTreasuryWithdrawals),
  `ProposalProcedure era` (tags 8 InvalidPrevGovActionId / 12
  DisallowedProposalDuringBootstrap).
- Conway UTXOW tag 10 (MissingRedeemers — PlutusPurpose AsItem).
- Conway UTXOS `CollectErrors`.
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
