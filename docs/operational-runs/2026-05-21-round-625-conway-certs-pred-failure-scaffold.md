---
title: "Round 625 ConwayCertsPredFailure scaffold + wire LEDGER tag 2 (A5 Phase-2.5)"
parent: Reference
---

# Round 625 ConwayCertsPredFailure scaffold + wire LEDGER tag 2 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `ConwayCertsPredFailure` 2-variant scaffold (the CERTS
sub-rule that Conway LEDGER tag 2 dispatches into — replaces
Shelley's DELEGS) and wires the parent variant to the typed
enum. After R625, only the Conway GOV sub-rule (LEDGER tag 3)
remains raw at the LEDGER level.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Certs.hs:114-119,163-178`
  (`data ConwayCertsPredFailure era` 2-variant ADT, CBOR encoder
  with tags 0/1).

## Changes

- Added `ConwayCertsPredFailure` 2-variant enum:
  - Tag 0 `WithdrawalsNotInRewardsCERTS(Withdrawals)` — R596
    Withdrawals reuse. Only emitted at protocol-version < 11 per
    upstream comment.
  - Tag 1 `CertFailure(Vec<u8>)` — raw pending
    `ConwayCertPredFailure` (nested CERT sub-rule decoder, which
    itself dispatches into DELEG/POOL/GOVCERT).
- `from_cbor` enforces 2-element envelope; unknown tags reject.
- Display routes typed payload through Withdrawals Display; raw
  CertFailure emits `<raw-cbor N bytes>`.
- Refactored `ConwayLedgerPredFailure::ConwayCertsFailure(Vec<u8>)`
  → `ConwayCertsFailure(ConwayCertsPredFailure)`. LEDGER tag 2
  dispatcher routes through typed decoder; tag 3 retains raw
  pending GOV decoder.

4 new focused unit tests:
- `_withdrawals_not_in_rewards_tag0` typed end-to-end.
- `_cert_failure_tag1` raw routing confirmation.
- `_ledger_pred_failure_certs_typed_routing_tag2` end-to-end
  LEDGER → CERTS → Withdrawals chain.
- `_unknown_tag_rejects` (tag 99).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (247 lib + 4
  doctests + 1 main, +4 new tests vs R624 baseline of 243)

## Remaining (A5 Phase-2.5+)

- Conway GOV sub-rule (LEDGER tag 3) — last raw variant at the
  Conway LEDGER root level.
- `ConwayCertPredFailure` (CERTS tag 1) — nested sub-rule
  dispatching into DELEG/POOL/GOVCERT.
- Conway UTXOW raw variants (tag 0 nested UTXO, 10/11/12/13/15/18).
- Conway UTXO sub-rule (referenced by UTXOW tag 0).
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
