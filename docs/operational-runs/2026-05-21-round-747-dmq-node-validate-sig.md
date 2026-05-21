---
title: "Round 747 dmq-node validate_sig entry point (dmq-node arc, slice 29)"
parent: Reference
---

# Round 747 dmq-node validate_sig entry point (dmq-node arc, slice 29)

Date: 2026-05-21

## Scope

Slice 29 of the dmq-node arc — the `validate_sig` entry point that
composes the five validator checks, completing the `SigSubmission`
validator.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `validate_sig` — runs the five checks in upstream `validateSig`
  order (KES period, pool eligibility, ocert-counter monotonicity,
  ocert signature, KES signature) against the
  `PoolValidationCtx`. The context's `ocert_map` update is committed
  only when every check passes; a failure rolls the context back —
  exactly as upstream's `exceptions` returns the unmodified state on
  `Left` (a working clone is committed only on success).
- `validate_sig_batch` — `traverse` over a signature list, threading
  the context; each result is `Ok(())` or `Err((SigId,
  SigValidationError))`, the context carrying forward only the
  accepted signatures' ocert-counter updates.
- `pool_id_of_cold_key` — the Blake2b-224 hash of the cold
  verification key (upstream `hashKey`), used as the
  `stake_map` / `ocert_map` key.

3 unit tests: failure on an uninitialized context, the context
rollback on a later (cryptographic) failure, and one result per
signature from the batch entry point.

## SigSubmission validator — complete

`validate_sig` composes the standalone checks shipped over R727
(KES period), R743 (ocert counter), R744 (pool eligibility), and
R746 (ocert / KES signatures). The `SigSubmission` validator is now
a complete, parity-cited port of upstream `Validate.hs`.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 115 lib (+3 vs R746's 112) +
  2 golden, all green.

## Remaining (dmq-node arc)

- `Configuration/Topology.hs`; the client / server protocol drivers;
  the `NodeKernel` / `Diffusion/*` run-loop wiring; `Tracer.hs`.
