---
title: "Round 690 Mark A5 typed-decoder arc complete in COMPLETION_ROADMAP (A6 doc hygiene)"
parent: Reference
---

# Round 690 Mark A5 typed-decoder arc complete in COMPLETION_ROADMAP (A6 doc hygiene)

Date: 2026-05-21

## Scope

Marks the A5 "cardano-submit-api structured rejection enum"
section of `docs/COMPLETION_ROADMAP.md` as code-complete, after
the R569-R688 typed predicate-failure decoder arc closed every
per-era variant.

## Changes (doc-only)

- A5 section header → `✅ CODE COMPLETE (verified 2026-05-21,
  R688)` with a summary of the completed typed decoder tree
  (Conway LEDGER 9/9 → UTXOW 19/19 / UTXO 23/23 / UTXOS 2/2 /
  CERT chain / GOV 19/19; the Shelley-family LEDGER tree; the
  `TxCert` / `GovAction` / `PParamsUpdate` / `CollectError` /
  `ContextError` leaf trees). Operator-soak certification noted
  as separately tracked under B2.
- Rewrote the trailing "Phase-2.5+ remaining work" line — the
  per-variant decoders are complete; the only remaining raw
  carrier is the deliberate `PParamValue::Raw` forward-compat
  fallback. The **Exit** criterion is marked ✅.

No source change — `docs/COMPLETION_ROADMAP.md` only.

## Validation

- `cargo fmt --all -- --check` — green.
- `cargo check-all` / `cargo lint` / `cargo test-all` —
  unaffected (doc-only edit).
- `check-parity-matrix.py` errors on a missing vendored
  reference-snapshot marker (`.reference-haskell-cardano-node/
  REFERENCE_TAG`) — a pre-existing environment condition (the
  vendored upstream tree is not fully provisioned in this
  environment) and unrelated to this edit, which touches
  `COMPLETION_ROADMAP.md` (not `parity-matrix.json`).

## Remaining (A6+)

- Filetree-description refinement.
- B2 operator-soak certification for `cardano-submit-api`.
