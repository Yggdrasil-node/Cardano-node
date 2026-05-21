---
title: "Round 648 Typed Conway GOV Voter variants (A5 Phase-2.5)"
parent: Reference
---

# Round 648 Typed Conway GOV Voter variants (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway GOV tags 5/9/13/14 (the Voter-bearing variants) by
adding the `Voter` enum and two carriers. After R648, **11 of
19 Conway GOV variants carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Procedures.hs:338-372`
  (`data Voter = CommitteeVoter (Credential HotCommitteeRole) |
  DRepVoter (Credential DRepRole) | StakePoolVoter (KeyHash
  StakePool)`; `decodeRecordSum` 2-array `[wire-tag, hash]` with
  wire-tags 0/1 = CommitteeVoter, 2/3 = DRepVoter, 4 =
  StakePoolVoter).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Gov.hs:186,194,207,209`
  (`DisallowedVoters`, `VotingOnExpiredGovAction`,
  `DisallowedVotesDuringBootstrap` carry `NonEmpty (Voter,
  GovActionId)`; `VotersDoNotExist` carries `NonEmpty Voter`).

## Changes

- Added `Voter` enum — 3 variants (CommitteeVoter / DRepVoter
  each carrying a `Credential`; StakePoolVoter carrying a
  `KeyHash`). `from_decoder` reconstructs the Credential variant
  from the wire-tag parity (even → KeyHashObj, odd →
  ScriptHashObj). Display matches upstream stock-derived Show.
- Added `NonEmptyVoter` carrier — `Vec<Voter>` (for tag 14).
- Added `NonEmptyVoterGovActionId` carrier — `Vec<(Voter,
  GovActionId)>` decoding a plain array of 2-element pairs (for
  tags 5/9/13). Display: `<head> :| [<tail>...]` with each pair
  as `(<Voter>, <GovActionId>)`.
- Refactored GOV variants 5/9/13 from `Vec<u8>` to
  `NonEmptyVoterGovActionId` and variant 14 to `NonEmptyVoter`.
  `from_cbor` decodes each 2-element envelope; Display routes
  the typed payloads. Empty NonEmpty lists reject at decode
  time.

3 new focused unit tests:
- `_voters_do_not_exist_tag14` — NonEmpty Voter (StakePoolVoter).
- `_disallowed_voters_tag5` — NonEmpty (Voter, GovActionId) pair
  with a DRepVoter (KeyHashObj).
- `_voters_do_not_exist_rejects_empty` — NonEmpty empty-array
  rejection.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (311 lib + 4
  doctests + 1 main, +3 new tests vs R647 baseline of 308)

## Remaining (A5 Phase-2.5+)

- Conway GOV raw variants (8) — typed decoders for `GovAction
  era` (tags 1/15 MalformedProposal / ZeroTreasuryWithdrawals),
  `ProposalProcedure era` (tags 8/12), `ProtVer` (tag 10
  ProposalCantFollow), `AccountAddress + Network` (tags 2/3),
  `NonEmpty (Credential HotCommitteeRole)` (tag 18).
- Conway UTXOW tag 10 (MissingRedeemers — PlutusPurpose AsItem).
- Conway UTXOS `CollectErrors`.
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
