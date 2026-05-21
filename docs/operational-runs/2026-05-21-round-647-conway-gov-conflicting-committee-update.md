---
title: "Round 647 Typed Conway GOV ConflictingCommitteeUpdate variant (A5 Phase-2.5)"
parent: Reference
---

# Round 647 Typed Conway GOV ConflictingCommitteeUpdate variant (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway GOV tag 6 (`ConflictingCommitteeUpdate`) by adding
the `NonEmptySetCredential` carrier. After R647, **7 of 19
Conway GOV variants carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Gov.hs:187-189,273-274`
  (`ConflictingCommitteeUpdate (NonEmptySet (Credential
  ColdCommitteeRole))`; CBOR `Sum` tag 6).

## Changes

- Added `NonEmptySetCredential` carrier — a `Vec<Credential>`
  preserving wire order. CBOR wire format is an optional CBOR
  tag 258 followed by an array of `Credential` 2-element items.
  Empty arrays reject at decode time. Display: `NonEmptySet
  (fromList [<Credential>, ...])`.
- Refactored `ConwayGovPredFailure::ConflictingCommitteeUpdate(Vec<u8>)`
  → `ConflictingCommitteeUpdate(NonEmptySetCredential)`.
  `from_cbor` decodes the 2-element envelope `[6, NonEmptySet
  Credential]`. Display routes the typed payload.

2 new focused unit tests:
- `_conflicting_committee_update_tag6` — single-Credential set
  (ScriptHashObj) with tag-258 tolerance.
- `_conflicting_committee_update_rejects_empty` — empty-set
  rejection.

Lint cleanup: removed a duplicated doc comment at the insertion
anchor.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (308 lib + 4
  doctests + 1 main, +2 new tests vs R646 baseline of 306)

## Remaining (A5 Phase-2.5+)

- Conway GOV raw variants (12) — typed decoders for `GovAction
  era`, `Voter`, `ProposalProcedure era`, `ProtVer`.
- Conway UTXOW tag 10 (MissingRedeemers — PlutusPurpose AsItem).
- Conway UTXOS `CollectErrors`.
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
