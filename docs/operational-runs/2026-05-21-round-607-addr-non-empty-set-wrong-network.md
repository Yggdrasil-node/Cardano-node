---
title: "Round 607 Addr + NonEmptySetAddr scaffold + wire UTXO tag 8 (A5 Phase-2.5)"
parent: Reference
---

# Round 607 Addr + NonEmptySetAddr scaffold + wire UTXO tag 8 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `Addr` wrapper + `NonEmptySetAddr` carrier and wires
`ShelleyUtxoPredFailure::WrongNetwork` (tag 8, 3-element envelope)
to typed struct variant. After R607, 8/11 UTXO variants carry
typed payloads.

The `Addr` Display currently emits a hex envelope; the full typed
Shelley vs Bootstrap address parse-tree split (PaymentCredential
+ StakeReference for Shelley; AttributedAddress for Byron Boot)
is deferred to a follow-on round.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs:178-180,238`
  (tag 8 `WrongNetwork Network (NonEmptySet Addr)` 3-element
  envelope).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Address.hs`
  (Addr sum + CBOR encoding as a single bytestring).

## Changes

- Added `Addr(Vec<u8>)` wrapper holding raw address bytes verbatim.
  `from_decoder` reads a CBOR bytestring item and rejects empty
  payloads. Display emits `Addr <hex N bytes: <hex>>` (interim
  format — full Shelley/Bootstrap typed Show lands in a follow-on
  round).
- Added `NonEmptySetAddr` struct (`BTreeSet<Addr>` for upstream
  byte-lex ordering) with both `from_cbor` and `from_decoder`
  entry points. Tag-258 tolerant, non-empty invariant enforced.
- Display: `NonEmptySet (fromList [<Addr>, ...])`.
- Refactored `ShelleyUtxoPredFailure::WrongNetwork(Vec<u8>)` →
  struct variant `WrongNetwork { expected: Network, wrongs:
  NonEmptySetAddr }`. Updated `tag()`, `constructor()`, Display
  routing, and `from_cbor` dispatcher (3-element envelope
  length validation enforced; mirrors R604's tag-9
  WrongNetworkWithdrawal pattern).

3 new focused unit tests:
- `_wrong_network_decodes_tag8` end-to-end with 1 address entry;
  verifies Network value, set size, address byte length and
  header byte, and full Display shape.
- `_wrong_network_rejects_wrong_envelope_length` validates the
  3-element invariant.
- `non_empty_set_addr_rejects_empty_set` (NonEmpty invariant).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (197 lib + 4
  doctests + 1 main, +3 new tests vs R606 baseline of 194)

## Remaining (A5 Phase-2.5+)

- Full typed `Addr` parse-tree (Shelley vs Bootstrap variant
  split, PaymentCredential + StakeReference for Shelley).
- UTXO raw tags pending: 5 (era-specific Value Mismatch), 6 / 10
  (NonEmpty TxOut era-specific).
- Wire typed `ShelleyUtxoPredFailure` into
  `ShelleyUtxowPredFailure::UtxoFailure(Vec<u8>)`.
- Wire typed `ShelleyUtxowPredFailure` into
  `ShelleyLedgerPredFailure::UtxowFailure(Vec<u8>)`.
- `ShelleyDelegsPredFailure` decoder for LEDGER tag 1.
- Mirror per-era predicate-failure tree for Allegra/Mary/Alonzo/
  Babbage/Conway eras.
