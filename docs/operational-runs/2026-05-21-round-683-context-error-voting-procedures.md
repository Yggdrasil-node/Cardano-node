---
title: "Round 683 Typed ContextError VotingProceduresFieldNotSupported (A5 Phase-2.5)"
parent: Reference
---

# Round 683 Typed ContextError VotingProceduresFieldNotSupported (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `Vote` enum and `VotingProcedure` type and types
`ContextError` tag 12 (`VotingProceduresFieldNotSupported`).
After R683, 7 of 8 `ContextError` variants are fully typed.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Procedures.hs:382-485`
  (`data Vote = VoteNo | VoteYes | Abstain`; `newtype
  VotingProcedures era = VotingProcedures (Map Voter (Map
  GovActionId (VotingProcedure era)))`; `data VotingProcedure
  era = VotingProcedure { vProcVote :: Vote, vProcAnchor ::
  StrictMaybe Anchor }`).

## Changes

- Added `Vote` enum (VoteNo / VoteYes / Abstain) — decodes the
  Word8 enum.
- Added `VotingProcedure { vote: Vote, anchor: Option<Anchor> }`
  — decodes the 2-element record `[vote, decodeNullStrictMaybe
  Anchor]`.
- Refactored `ContextError::VotingProceduresFieldNotSupported(Vec<u8>)`
  → `VotingProceduresFieldNotSupported(Vec<(Voter, Vec<(GovActionId,
  VotingProcedure)>)>)` — the nested `Map Voter (Map GovActionId
  VotingProcedure)`.
- `ContextError::from_decoder` special-cases tag 12: decodes the
  outer voter map and each inner gov-action map.
- Display renders the nested `fromList` maps.

1 new focused unit test:
- `context_error_decodes_voting_procedures_field` — a tag-12
  `ContextError` with one voter / one gov-action / one
  `VoteYes` voting procedure.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (350 lib + 4
  doctests + 1 main, +1 new test vs R682 baseline of 349)

## Remaining (A5 Phase-2.5+)

- `ContextError` tag 8 (`BabbageContextError` — the inherited
  Babbage-era context-error tree).
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
