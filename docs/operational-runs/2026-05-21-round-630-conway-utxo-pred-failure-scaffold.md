---
title: "Round 630 ConwayUtxoPredFailure scaffold + wire UTXOW tag 0 (A5 Phase-2.5)"
parent: Reference
---

# Round 630 ConwayUtxoPredFailure scaffold + wire UTXOW tag 0 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `ConwayUtxoPredFailure` 23-variant scaffold (the largest
sub-rule enum in the Conway predicate-failure tree) and wires
`ConwayUtxowPredFailure::UtxoFailure` to the typed enum. After
R630, **12 of 23 Conway UTXO variants carry typed payloads** by
reusing existing carriers; the 11 remaining variants keep raw
inner CBOR pending Value / ExUnits / ValidityInterval /
DeltaCoin / NonEmptyMap decoders.

The Conway LEDGER → UTXOW → UTXO chain now renders typed
end-to-end through 12 UTXO leaves.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Utxo.hs:73-144,311-335`
  (`data ConwayUtxoPredFailure era` 23-variant ADT + CBOR
  encoder; tags 0-22).

## Changes

- Added `ConwayUtxoPredFailure` 23-variant enum mirroring
  upstream:
  - **Typed (12 variants):**
    - Tag 1 `BadInputsUTxO(NonEmptySetTxIn)` (R603 reuse).
    - Tag 3 `MaxTxSizeUTxO(Mismatch<u64>)` — Word32 via ToGroup.
    - Tag 4 `InputSetEmptyUTxO` — no payload.
    - Tag 5 `FeeTooSmallUTxO(Mismatch<u64>)` — Coin via ToGroup,
      expected-first per `swapMismatch`.
    - Tag 7 `WrongNetwork { expected, wrongs }` —
      Network + NonEmptySetAddr.
    - Tag 8 `WrongNetworkWithdrawal { expected, wrongs }` —
      Network + NonEmptySetAccountAddress.
    - Tag 9 `OutputTooSmallUTxO(NonEmptyTxOut)` (R620 reuse).
    - Tag 10 `OutputBootAddrAttrsTooBig(NonEmptyTxOut)`.
    - Tag 16 `WrongNetworkInTxBody(Mismatch<Network>)` — via
      ToGroup, expected-first.
    - Tag 17 `OutsideForecast(u64)` — SlotNo.
    - Tag 18 `TooManyCollateralInputs(Mismatch<u64>)` — Word16
      via ToGroup, expected-first.
    - Tag 19 `NoCollateralInputs` — no payload.
  - **Raw (11 variants):** tags 0 (UTXOS sub-rule), 2
    (ValidityInterval), 6 (Value), 11 (Int/Int/TxOut triple),
    12/20 (DeltaCoin+Coin), 13 (NonEmptyMap), 14 (ExUnits), 15
    (Value), 21 (TxOut+Coin pair), 22 (NonEmpty TxIn).
- `from_cbor` enforces exact envelope length per variant (1 for
  no-payload tags 4/19; 2 for most; 3 for ToGroup/multi-arg
  tags). Tags 5/16/18 use `swapMismatch` expected-first wire
  ordering. Unknown tags reject.
- Display routes 12 typed payloads through their typed Display;
  raw variants emit `<Constructor> <raw-cbor N bytes>`.
- Refactored `ConwayUtxowPredFailure::UtxoFailure(Vec<u8>)` →
  `UtxoFailure(ConwayUtxoPredFailure)`. UTXOW tag 0 dispatcher
  routes through the typed decoder; Display routes the typed
  nested payload.

9 new focused unit tests:
- `_input_set_empty_tag4` / `_no_collateral_inputs_tag19` —
  no-payload variants.
- `_max_tx_size_tag3` — Word32 Mismatch via ToGroup.
- `_fee_too_small_tag5` — Coin Mismatch with swapMismatch
  expected-first ordering.
- `_bad_inputs_tag1` — NonEmptySetTxIn with tag-258 tolerance.
- `_outside_forecast_tag17` — SlotNo typed.
- `_routes_pending_to_raw_tag6` — raw routing confirmation.
- `_unknown_tag_rejects` — tag 99 rejection.
- `conway_utxow_pred_failure_utxo_typed_routing_tag0` —
  end-to-end UTXOW → UTXO chain.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (273 lib + 4
  doctests + 1 main, +9 new tests vs R629 baseline of 264)

## Remaining (A5 Phase-2.5+)

- Conway UTXO raw variants — typed decoders for Value, ExUnits,
  ValidityInterval, DeltaCoin, NonEmptyMap TxIn TxOut, the
  Int/Int/TxOut triple, the TxOut/Coin pair, and NonEmpty TxIn.
- Conway UTXOS sub-rule (referenced by UTXO tag 0) — itself a
  Plutus-script-evaluation predicate-failure chain.
- Conway UTXOW raw variants (tags 10/11/12/13/15/18).
- Conway GOV raw variants (18 governance-specific decoders).
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
