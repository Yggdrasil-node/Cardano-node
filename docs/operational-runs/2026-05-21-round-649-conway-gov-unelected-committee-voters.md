---
title: "Round 649 Typed Conway GOV UnelectedCommitteeVoters variant (A5 Phase-2.5)"
parent: Reference
---

# Round 649 Typed Conway GOV UnelectedCommitteeVoters variant (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway GOV tag 18 (`UnelectedCommitteeVoters`) by adding
the `NonEmptyCredential` list carrier. After R649, **12 of 19
Conway GOV variants carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Gov.hs:216-217,255,297-298`
  (`UnelectedCommitteeVoters (NonEmpty (Credential
  HotCommitteeRole))`; CBOR `Sum` tag 18).

## Changes

- Added `NonEmptyCredential` carrier — a `Vec<Credential>`
  preserving wire order (unlike `NonEmptySetCredential`'s
  tag-258 set form). CBOR wire format is a plain CBOR array;
  empty arrays reject at decode time. Display: `<head> :|
  [<tail>...]`.
- Refactored `ConwayGovPredFailure::UnelectedCommitteeVoters(Vec<u8>)`
  → `UnelectedCommitteeVoters(NonEmptyCredential)`. `from_cbor`
  decodes the 2-element envelope `[18, NonEmpty Credential]`.
  Display routes the typed payload.

2 new focused unit tests:
- `_unelected_committee_voters_tag18` — single-Credential
  NonEmpty (KeyHashObj).
- `_unelected_committee_voters_rejects_empty` — NonEmpty
  empty-array rejection.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (313 lib + 4
  doctests + 1 main, +2 new tests vs R648 baseline of 311)

## Remaining (A5 Phase-2.5+)

- Conway GOV raw variants (7) — typed decoders for `GovAction
  era` (tags 1/15), `ProposalProcedure era` (tags 8/12),
  `ProtVer` (tag 10), `AccountAddress + Network` (tags 2/3).
- Conway UTXOW tag 10 (MissingRedeemers — PlutusPurpose AsItem).
- Conway UTXOS `CollectErrors`.
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
