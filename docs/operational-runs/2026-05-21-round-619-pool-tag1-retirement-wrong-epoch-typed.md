---
title: "Round 619 POOL tag 1 StakePoolRetirementWrongEpochPOOL typed (A5 Phase-2.5)"
parent: Reference
---

# Round 619 POOL tag 1 StakePoolRetirementWrongEpochPOOL typed (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Wires `ShelleyPoolPredFailure::StakePoolRetirementWrongEpochPOOL`
(tag 1) to a typed struct variant matching upstream's flattened
2-Mismatch encoding. **Closes all 6 POOL variants — the Shelley
LEDGER predicate-failure tree is now structurally typed
end-to-end.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Pool.hs:96-98,158-163,180-189`
  (variant ADT, CBOR encoder for tag 1, and the symmetric
  `decCBOR` that reconstructs `(Mismatch ltSupplied gtExpected)
  (Mismatch ltSupplied ltExpected)` from `[1, gtExpected,
  ltSupplied, ltExpected]`).

## Changes

- Refactored `ShelleyPoolPredFailure::StakePoolRetirementWrongEpochPOOL(Vec<u8>)`
  → struct variant `{ supplied: u64, gt_expected: u64,
  lt_expected: u64 }`. Captures the 3 distinct EpochNo fields
  from upstream's flattened encoding directly (rather than
  duplicating the shared `supplied` field across two Mismatch
  records).
- `from_cbor` dispatcher now decodes the 4-element envelope
  `[1, gt_expected, supplied, lt_expected]` into the typed
  struct, enforcing exact envelope length.
- Display reconstructs the upstream pair of Mismatches via two
  inner `Mismatch<u64>` records sharing the `supplied` field:
  `StakePoolRetirementWrongEpochPOOL (Mismatch (RelGT)
  {supplied, expected: gt_expected}) (Mismatch (RelLTEQ)
  {supplied, expected: lt_expected})`.
- Replaced R616's `_retirement_wrong_epoch_stays_raw_tag1` test
  with the typed end-to-end `_retirement_wrong_epoch_decodes_tag1`
  variant: asserts the 3 EpochNo fields decode correctly and the
  Display reproduces the upstream Show envelope (RelGT + RelLTEQ
  paired Mismatches).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (227 lib + 4
  doctests + 1 main, net 0 vs R618 baseline of 227 — replaced 1
  raw-routing test with the typed equivalent).

## Status of Shelley LEDGER predicate-failure tree

**The Shelley LEDGER predicate-failure tree is now structurally
typed end-to-end.** Coverage by sub-rule:
- LEDGER: 4/4 (R596 R597 R611 R612 chain).
- UTXOW: 11/11 (R598 R599 R600 R601 R610 chain).
- UTXO: 11/11 (R602 R603 R604 R605 R607 R608 R609 chain).
- PPUP: 3/3 (R605 R606 chain).
- DELEGS → DELPL: full chain wired through R613.
- DELEGS → DELPL → POOL: 6/6 (R614 R616 R619 chain).
- DELEGS → DELPL → DELEG: 16/16 (R615 R617 R618 chain).

## Remaining (A5 Phase-2.5+)

- Inner per-TxOut typed parse (era-specific Shelley/Babbage
  shapes) — used by UTXO tags 6/10 typed wrappers (R609 captures
  raw bytes via `NonEmptyTxOut` carrier).
- Full typed `Addr` Show parse (Shelley vs Bootstrap split) — used
  by UTXO tag 8 typed wrapper (R607 captures raw bytes via
  `NonEmptySetAddr` carrier).
- Mirror the per-era predicate-failure tree for Allegra / Mary /
  Alonzo / Babbage / Conway eras (separate enum trees with their
  own per-era variant additions).
