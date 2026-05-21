---
title: "Round 653 Conway ProposalProcedure scaffold closes Conway GOV (A5 Phase-2.5)"
parent: Reference
---

# Round 653 Conway ProposalProcedure scaffold closes Conway GOV (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `Anchor` and `ProposalProcedure` types and wires Conway
GOV tags 8 (`InvalidPrevGovActionId`) and 12
(`DisallowedProposalDuringBootstrap`). **After R653, all 19
Conway GOV variants carry typed payloads — the Conway GOV
sub-rule is fully typed.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Procedures.hs:498-545`
  (`data ProposalProcedure era = ProposalProcedure {
  pProcDeposit :: Coin, pProcReturnAddr :: AccountAddress,
  pProcGovAction :: GovAction era, pProcAnchor :: Anchor }`;
  CBOR 4-element record array).

## Changes

- Added `Anchor { url: String, data_hash: [u8; 32] }` — decodes
  the 2-element record array `[url-text, 32-byte-hash]`. Display:
  `Anchor {anchorUrl = Url {urlToText = "<url>"}, anchorDataHash
  = SafeHash "<hex>"}`.
- Added `ProposalProcedure { deposit: u64, return_addr:
  RewardAccount, gov_action: GovAction, anchor: Anchor }` —
  decodes the 4-element record array. The nested `GovAction` is
  decoded through R652's `GovAction::from_decoder`. Display
  matches the stock-derived record Show.
- Refactored `ConwayGovPredFailure::InvalidPrevGovActionId(Vec<u8>)`
  → `InvalidPrevGovActionId(ProposalProcedure)` and
  `DisallowedProposalDuringBootstrap(Vec<u8>)` →
  `DisallowedProposalDuringBootstrap(ProposalProcedure)`.
- Removed the now-unused `capture_raw` closure from
  `ConwayGovPredFailure::from_cbor` — all 19 variants are typed.

1 new focused unit test:
- `_invalid_prev_gov_action_id_tag8` — full ProposalProcedure
  round-trip (deposit + return-addr + GovAction InfoAction +
  Anchor), asserting the nested record Display.

## Conway predicate-failure tree status

The Conway predicate-failure tree is now structurally typed
across every sub-rule:
- LEDGER: 9/9.
- UTXOW: 17/19 (only tag 10 MissingRedeemers raw).
- UTXO: 23/23.
- UTXOS: 1/2 (only CollectErrors raw).
- CERTS → CERT: DELEG 8/8, POOL 6/6, GOVCERT 6/6.
- GOV: **19/19** (closed by R653).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (319 lib + 4
  doctests + 1 main, +1 new test vs R652 baseline of 318)

## Remaining (A5 Phase-2.5+)

- GovAction per-variant typed payloads (PParamsUpdate,
  Constitution, UnitInterval, treasury-withdrawal maps).
- Conway UTXOW tag 10 (MissingRedeemers — PlutusPurpose AsItem).
- Conway UTXOS `CollectErrors`.
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
