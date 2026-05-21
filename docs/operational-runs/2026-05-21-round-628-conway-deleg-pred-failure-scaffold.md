---
title: "Round 628 ConwayDelegPredFailure scaffold + wire CERT tag 1 (A5 Phase-2.5)"
parent: Reference
---

# Round 628 ConwayDelegPredFailure scaffold + wire CERT tag 1 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `ConwayDelegPredFailure` 8-variant scaffold (the DELEG
sub-rule under CERT tag 1; Conway-era stake/DRep delegation
predicate failures) and wires the parent variant to the typed
enum. **All 8 Conway DELEG variants carry fully-typed payloads.**

After R628, the Conway LEDGER → CERTS → CERT → DELEG chain
renders typed end-to-end through all 8 DELEG leaves.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Deleg.hs:105-114,124-154`
  (`data ConwayDelegPredFailure era` 8-variant ADT — tags 1-8;
  upstream skips tag 0).

## Changes

- Added `ConwayDelegPredFailure` 8-variant enum mirroring upstream:
  - Tag 1 `IncorrectDepositDELEG(u64)` — Coin.
  - Tag 2 `StakeKeyRegisteredDELEG(Credential)`.
  - Tag 3 `StakeKeyNotRegisteredDELEG(Credential)`.
  - Tag 4 `StakeKeyHasNonZeroAccountBalanceDELEG(u64)`.
  - Tag 5 `DelegateeDRepNotRegisteredDELEG(Credential)`.
  - Tag 6 `DelegateeStakePoolNotRegisteredDELEG(KeyHash)`.
  - Tag 7 `DepositIncorrectDELEG(Mismatch<u64>)`.
  - Tag 8 `RefundIncorrectDELEG(Mismatch<u64>)`.
- Tags 7/8 use the nested 2-array Mismatch encoding (`To mm` in
  upstream's encoder) — distinct from Conway LEDGER tags 5/6 +
  GOV tag 4 which use ToGroup-flattened encoding.
- `from_cbor` enforces 2-element envelope; unknown tags
  (including upstream-skipped tag 0) reject.
- Display routes all 8 typed payloads through their typed inner
  Display (Coin via CoinShow, Credential via R618 typed
  KeyHashObj/ScriptHashObj, KeyHash via record-Show, Mismatch
  via Mismatch<CoinShow>).
- Refactored `ConwayCertPredFailure::DelegFailure(Vec<u8>)` →
  `DelegFailure(ConwayDelegPredFailure)`. CERT `from_cbor`
  decodes through typed DELEG. Display routes the typed nested
  payload.

Test surface updates:
- Updated R625's `_cert_failure_tag1` test (the CERTS → CERT →
  DELEG chain test) to use a typed inner DELEG payload
  (IncorrectDepositDELEG coin=100) and assert the typed nested
  Display chain.
- Updated R627's `_deleg_failure_raw_routing_tag1` (now `_typed_routing_tag1`) to use a
  typed inner DELEG payload.

5 new focused unit tests for DELEG:
- `_incorrect_deposit_tag1` Coin typed.
- `_stake_key_registered_tag2` Credential typed (KeyHashObj).
- `_delegatee_stake_pool_not_registered_tag6` KeyHash typed.
- `_deposit_incorrect_tag7` Mismatch<u64> via nested 2-array.
- `_unknown_tag_rejects` (tag 0 — upstream skip).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (259 lib + 4
  doctests + 1 main, +5 net new tests vs R627 baseline of 254)

## Remaining (A5 Phase-2.5+)

- `ConwayGovCertPredFailure` (CERT tag 3) — GOVCERT sub-rule for
  governance certificates (DRep registration, committee
  hot/cold authorization).
- Conway UTXOW raw variants (tag 0 nested UTXO, 10/11/12/13/15/18).
- Conway UTXO sub-rule (referenced by UTXOW tag 0).
- Conway GOV raw variants (18 governance-specific decoders).
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
