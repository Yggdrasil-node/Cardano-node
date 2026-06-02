---
title: "Round 689 PARITY_SUMMARY update for the A5 typed-rejection arc (A6 doc hygiene)"
parent: Reference
---

# Round 689 PARITY_SUMMARY update for the A5 typed-rejection arc (A6 doc hygiene)

Date: 2026-05-21

## Scope

Records the completed R569-R688 `cardano-submit-api` typed
predicate-failure decoder arc in the living `docs/PARITY_SUMMARY.md`
status doc (A6 "Workspace + documentation hygiene" backlog
item).

## Rationale

`docs/PARITY_SUMMARY.md` is a living status doc (per the root `AGENTS.md`
living-status policy). Its journal was current only through
R568; the 120-round A5 cardano-submit-api typed-rejection arc
(R569-R688) was unrecorded, so the doc understated the
implemented surface.

## Changes (doc-only)

- Updated the `Prepared` / `Status` header lines from "post-R568"
  to "post-R688".
- Added an `R569–R688 cardano-submit-api typed-rejection
  decoder arc` narrative paragraph summarising the completed
  per-era predicate-failure decoder tree: the Conway LEDGER
  tree (9/9 + UTXOW 19/19 + UTXO 23/23 + UTXOS 2/2 + CERT
  chain + GOV 19/19), the Shelley-family LEDGER tree, the deep
  leaf types (era-tolerant `TxOut`, Byron bootstrap address,
  `TxCert` / `GovAction` / `PParamsUpdate` / `CollectError` /
  `ContextError` trees), and the `EraApplyTxError` /
  `TxValidationErrorInCardanoMode` end-to-end wiring.

No source change — `docs/PARITY_SUMMARY.md` only.

## Validation

- `cargo fmt --all -- --check` — green.
- `cargo check-all` / `cargo lint` / `cargo test-all` —
  unaffected (doc-only edit, no source touched).
- `check-strict-mirror.py` — unaffected (no source-file move).
- `check-stale-placement.py` exits 1 both with and without
  this edit — a pre-existing condition (vendored
  `.reference-haskell-cardano-node/deps/*/.git` nested git
  metadata), not introduced by this round (verified via a
  stash/pop A-B comparison).

## Remaining (A6+)

- Filetree-description refinement (`cardano-filetree-maintainer`).
