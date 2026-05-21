---
title: "Round 629 ConwayGovCertPredFailure scaffold + wire CERT tag 3 (A5 Phase-2.5)"
parent: Reference
---

# Round 629 ConwayGovCertPredFailure scaffold + wire CERT tag 3 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `ConwayGovCertPredFailure` 6-variant scaffold (the
GOVCERT sub-rule under CERT tag 3 — DRep registration/refund and
committee hot/cold authorization predicates) and wires the parent
variant to the typed enum. **All 6 GOVCERT variants carry
fully-typed payloads.**

This closes the **entire Conway CERT sub-rule** (3/3 typed):
- DELEG: 8/8 typed (R628).
- POOL: 6/6 typed via Shelley reuse (R616/R619 + R627 wiring).
- GOVCERT: 6/6 typed (R629).

The Conway LEDGER → CERTS → CERT → {DELEG, POOL, GOVCERT} chain
renders typed end-to-end through every leaf.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/GovCert.hs:108-118,128-146`
  (`data ConwayGovCertPredFailure era` 6-variant ADT — tags 0-5;
  tags 2/4 use ToGroup-flattened Mismatch encoding).

## Changes

- Added `ConwayGovCertPredFailure` 6-variant enum mirroring
  upstream:
  - Tag 0 `ConwayDRepAlreadyRegistered(Credential)`.
  - Tag 1 `ConwayDRepNotRegistered(Credential)`.
  - Tag 2 `ConwayDRepIncorrectDeposit(Mismatch<u64>)` — ToGroup
    flattened.
  - Tag 3 `ConwayCommitteeHasPreviouslyResigned(Credential)`.
  - Tag 4 `ConwayDRepIncorrectRefund(Mismatch<u64>)` — ToGroup
    flattened.
  - Tag 5 `ConwayCommitteeIsUnknown(Credential)`.
- `from_cbor` dispatches via two shared helper closures
  (`credential_variant` for tags 0/1/3/5 with 2-element
  envelopes; `togroup_coin_mismatch` for tags 2/4 with 3-element
  flattened envelopes). Unknown tags reject.
- Display routes all 6 typed payloads through their typed inner
  Display (Credential via R618 KeyHashObj/ScriptHashObj,
  Mismatch via Mismatch<CoinShow>).
- Refactored `ConwayCertPredFailure::GovCertFailure(Vec<u8>)` →
  `GovCertFailure(ConwayGovCertPredFailure)`. CERT `from_cbor`
  decodes through typed GOVCERT. Display routes the typed
  nested payload.

5 new focused unit tests:
- `_drep_already_registered_tag0` Credential typed (KeyHashObj).
- `_drep_incorrect_deposit_tag2` Mismatch via ToGroup flattened.
- `_committee_resigned_tag3` Credential typed (ScriptHashObj).
- `_gov_cert_failure_typed_routing_tag3` end-to-end CERT →
  GOVCERT chain.
- `_unknown_tag_rejects` (tag 99).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (264 lib + 4
  doctests + 1 main, +5 new tests vs R628 baseline of 259)

## Remaining (A5 Phase-2.5+)

- Conway UTXOW raw variants (tag 0 nested UTXO, 10/11/12/13/15/18).
- Conway UTXO sub-rule (referenced by UTXOW tag 0).
- Conway GOV raw variants (18 governance-specific decoders for
  GovActionId, GovAction, Voter, ProposalProcedure, etc.).
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
