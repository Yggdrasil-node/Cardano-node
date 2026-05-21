---
title: "Round 626 ConwayGovPredFailure scaffold + wire LEDGER tag 3 (A5 Phase-2.5)"
parent: Reference
---

# Round 626 ConwayGovPredFailure scaffold + wire LEDGER tag 3 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `ConwayGovPredFailure` 19-variant scaffold (the GOV
sub-rule, new in Conway for governance actions) and wires Conway
LEDGER tag 3 to the typed enum. After R626, **all 9 Conway
LEDGER root variants carry typed payloads at one level of
nesting** — every LEDGER root tag now has a structurally-typed
Rust value at its immediate sub-rule.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Gov.hs:177-218,235-298`
  (`data ConwayGovPredFailure era` 19-variant ADT, CBOR encoder
  with tags 0-18).

## Changes

- Added `ConwayGovPredFailure` 19-variant enum mirroring upstream:
  - **Typed (1 variant):** Tag 4 `ProposalDepositIncorrect(Mismatch<u64>)` — Mismatch RelEQ Coin via ToGroup flattened.
  - **Raw (18 variants):** tags 0/1/2/3/5/6/7/8/9/10/11/12/13/14/15/16/17/18 — pending governance-specific decoders for
    GovActionId, GovAction, Voter, ProposalProcedure, ProtVer,
    Credential roles (ColdCommitteeRole, HotCommitteeRole),
    AccountAddress, StrictMaybe ScriptHash, StrictMaybe
    (GovPurposeId 'HardForkPurpose).
- `from_cbor` enforces exact envelope length per variant:
  - 2-element: tags 0/1/5-9, 12-18.
  - 3-element: tags 2/3/4/11.
  - 4-element: tag 10 (StrictMaybe + ToGroup Mismatch).
- Display: tag 4 routes through typed `Mismatch<CoinShow>`;
  remaining 18 variants emit `<Constructor> <raw-cbor N bytes>`.
- Refactored
  `ConwayLedgerPredFailure::ConwayGovFailure(Vec<u8>)` →
  `ConwayGovFailure(ConwayGovPredFailure)`. LEDGER tag 3
  dispatcher routes through the typed decoder.

4 new focused unit tests:
- `_proposal_deposit_incorrect_tag4` typed end-to-end with
  ToGroup-flattened Mismatch.
- `_routes_pending_to_raw_tag0` raw fallback confirmation
  (GovActionsDoNotExist).
- `_ledger_pred_failure_gov_typed_routing_tag3` end-to-end
  LEDGER → GOV chain.
- `_unknown_tag_rejects` (tag 99).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (251 lib + 4
  doctests + 1 main, +4 new tests vs R625 baseline of 247)

## Remaining (A5 Phase-2.5+)

- GOV raw variants — typed governance decoders:
  - `GovActionId`, `GovAction era`, `Voter`,
    `ProposalProcedure era`, `ProtVer`,
    `StrictMaybe ScriptHash`, `StrictMaybe (GovPurposeId
    'HardForkPurpose)`.
- `ConwayCertPredFailure` (CERTS tag 1) — nested DELEG/POOL/GOVCERT
  dispatcher.
- Conway UTXOW raw variants (tag 0 nested UTXO, 10/11/12/13/15/18).
- Conway UTXO sub-rule (referenced by UTXOW tag 0).
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
