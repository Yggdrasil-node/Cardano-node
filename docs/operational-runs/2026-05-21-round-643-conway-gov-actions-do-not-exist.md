---
title: "Round 643 Typed Conway GOV GovActionsDoNotExist variant (A5 Phase-2.5)"
parent: Reference
---

# Round 643 Typed Conway GOV GovActionsDoNotExist variant (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Opens typed Conway GOV variant coverage with the `GovActionId`
type and wires GOV tag 0 (`GovActionsDoNotExist`). After R643,
**2 of 19 Conway GOV variants carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Procedures.hs:162-197`
  (`newtype GovActionIx = GovActionIx {unGovActionIx :: Word16}`;
  `data GovActionId = GovActionId { gaidTxId :: TxId,
  gaidGovActionIx :: GovActionIx }`; CBOR `Rec GovActionId !>
  To gaidTxId !> To gaidGovActionIx` = 2-element record array).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Gov.hs:178,261-262`
  (`GovActionsDoNotExist (NonEmpty GovActionId)`, CBOR `Sum`
  tag 0).

## Changes

- Added `GovActionId { tx_id: TxId, gov_action_ix: u16 }` —
  decodes the 2-element record array `[txid, govActionIx]`.
  Display matches the stock-derived record Show:
  `GovActionId {gaidTxId = TxId {unTxId = SafeHash "<hex>"},
  gaidGovActionIx = GovActionIx {unGovActionIx = <n>}}`.
- Added `NonEmptyGovActionId` carrier — a `Vec<GovActionId>`
  preserving wire order; empty arrays reject at decode time.
  Display: `<head> :| [<tail>...]`.
- Refactored `ConwayGovPredFailure::GovActionsDoNotExist(Vec<u8>)`
  → `GovActionsDoNotExist(NonEmptyGovActionId)`. `from_cbor`
  decodes the 2-element envelope `[0, NonEmpty GovActionId]`.
  Display routes the typed payload.

2 new tests + 1 replaced:
- Replaced R626's `_routes_pending_to_raw_tag0` with the typed
  `_gov_actions_do_not_exist_tag0` (single-GovActionId
  round-trip + Display).
- New `_gov_actions_do_not_exist_rejects_empty` — NonEmpty
  empty-array rejection.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (300 lib + 4
  doctests + 1 main, +1 net new test vs R642 baseline of 299 —
  added 2, replaced 1)

## Remaining (A5 Phase-2.5+)

- Conway GOV raw variants — typed governance decoders for
  `GovAction era`, `Voter`, `ProposalProcedure era`, `ProtVer`,
  `Credential` cold/hot committee roles, `AccountAddress`,
  `StrictMaybe ScriptHash`, `StrictMaybe (GovPurposeId
  'HardForkPurpose)`, `NonEmptyMap`.
- Conway UTXOW tag 10 (MissingRedeemers — PlutusPurpose AsItem).
- Conway UTXOS `CollectErrors`.
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
