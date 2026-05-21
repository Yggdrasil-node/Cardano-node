---
title: "Round 655 Conway UTXOW MissingRedeemers closes Conway sub-rule tree (A5 Phase-2.5)"
parent: Reference
---

# Round 655 Conway UTXOW MissingRedeemers closes Conway sub-rule tree (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway UTXOW tag 10 (`MissingRedeemers`) — the last raw
variant in the entire Conway predicate-failure sub-rule tree —
by adding the `ConwayPlutusPurposeItem` enum and the
`NonEmptyMissingRedeemer` carrier.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Utxow.hs`
  (`MissingRedeemers (NonEmpty (PlutusPurpose AsItem era,
  ScriptHash))`; CBOR `Sum` tag 10).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Scripts.hs`
  (`ConwayPlutusPurpose f era` — the `f = AsItem` form carries
  the actual purpose item; CBORGroup `[word8-tag, item]`, tags
  0-5).

## Changes

- Added `ConwayPlutusPurposeItem` 6-variant enum (the AsItem
  form of the Plutus purpose):
  - `ConwaySpending(TxIn)` — typed.
  - `ConwayMinting(PolicyId)` — typed.
  - `ConwayCertifying(Vec<u8>)` — raw, pending the per-era
    `TxCert` decoder.
  - `ConwayRewarding(RewardAccount)` — typed.
  - `ConwayVoting(Voter)` — typed.
  - `ConwayProposing(ProposalProcedure)` — typed.
  - `from_decoder` reads the 2-element CBORGroup `[tag, item]`;
    the `ConwayCertifying` item is captured raw by byte range
    via the ledger decoder's recursive `skip()`.
  - Display matches upstream `Show (ConwayPlutusPurpose AsItem
    era)`: `<Constructor> (AsItem {unAsItem = <item>})`.
- Added `NonEmptyMissingRedeemer` carrier — `Vec<(ConwayPlutusPurposeItem,
  ScriptHash)>` decoding a plain array of 2-element pairs;
  empty arrays reject at decode time.
- Refactored `ConwayUtxowPredFailure::MissingRedeemers(Vec<u8>)`
  → `MissingRedeemers(NonEmptyMissingRedeemer)`. `from_cbor`
  decodes the 2-element envelope `[10, NonEmpty (purpose,
  hash)]`.
- Removed the now-unused `capture_raw` closure from
  `ConwayUtxowPredFailure::from_cbor` — all 19 UTXOW variants
  typed.

2 new tests + 1 replaced:
- Replaced R624's `_routes_pending_to_raw_tag10` with the typed
  `_missing_redeemers_tag10` (ConwaySpending + ScriptHash pair
  round-trip).
- New `_missing_redeemers_rejects_empty` — NonEmpty empty-array
  rejection.

## Conway predicate-failure tree — fully typed

Every Conway predicate-failure sub-rule now carries typed
payloads end-to-end:
- LEDGER 9/9, UTXOW **19/19** (closed by R655), UTXO 23/23,
  UTXOS 2/2, CERTS → CERT (DELEG 8/8, POOL 6/6, GOVCERT 6/6),
  GOV 19/19.

Only the deepest leaf payloads remain raw within their typed
carriers: `TxCert` (ConwayCertifying / per-era certificate
ADT), `PParamsUpdate` / `Constitution` / `UnitInterval`
(GovAction variants), `ContextError` (CollectError
BadTranslation), and `PlutusPurpose AsItem` for CollectError
NoRedeemer.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (321 lib + 4
  doctests + 1 main, +1 net new test vs R654 baseline of 320 —
  added 2, replaced 1)

## Remaining (A5 Phase-2.5+)

- Deepest leaf payloads: `TxCert`, `PParamsUpdate`,
  `Constitution`, `UnitInterval`, `ContextError`.
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
