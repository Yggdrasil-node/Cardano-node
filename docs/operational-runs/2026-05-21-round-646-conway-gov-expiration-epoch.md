---
title: "Round 646 Typed Conway GOV ExpirationEpochTooSmall variant (A5 Phase-2.5)"
parent: Reference
---

# Round 646 Typed Conway GOV ExpirationEpochTooSmall variant (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway GOV tag 7 (`ExpirationEpochTooSmall`) by adding the
`NonEmptyMapCredentialEpoch` carrier. After R646, **6 of 19
Conway GOV variants carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Gov.hs:190-192,275-276`
  (`ExpirationEpochTooSmall (NonEmptyMap (Credential
  ColdCommitteeRole) EpochNo)`; CBOR `Sum` tag 7).
- `.reference-haskell-cardano-node/deps/cardano-base/cardano-slotting/src/Cardano/Slotting/Slot.hs:118-120`
  (`newtype EpochNo = EpochNo {unEpochNo :: Word64}` with
  `deriving (Show) via Quiet` → `EpochNo <n>`).

## Changes

- Added `NonEmptyMapCredentialEpoch` carrier — a `Vec<(Credential,
  u64)>` preserving wire order. CBOR wire format is a CBOR map
  (Credential key → EpochNo value). Empty maps reject at decode
  time. Display: `NonEmptyMap (fromList [(<Credential>,EpochNo
  <n>),...])`.
- Refactored `ConwayGovPredFailure::ExpirationEpochTooSmall(Vec<u8>)`
  → `ExpirationEpochTooSmall(NonEmptyMapCredentialEpoch)`.
  `from_cbor` decodes the 2-element envelope `[7, NonEmptyMap
  Credential EpochNo]`. Display routes the typed payload.

2 new focused unit tests:
- `_expiration_epoch_too_small_tag7` — single-entry map
  round-trip asserting the typed Display.
- `_expiration_epoch_too_small_rejects_empty` — empty-map
  rejection.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (306 lib + 4
  doctests + 1 main, +2 new tests vs R645 baseline of 304)

## Remaining (A5 Phase-2.5+)

- Conway GOV raw variants (13) — typed decoders for `GovAction
  era`, `Voter`, `ProposalProcedure era`, `ProtVer`,
  `Credential` cold/hot committee role NonEmptySets.
- Conway UTXOW tag 10 (MissingRedeemers — PlutusPurpose AsItem).
- Conway UTXOS `CollectErrors`.
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
