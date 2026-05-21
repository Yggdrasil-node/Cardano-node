---
title: "Round 743 dmq-node ocert-counter validation (dmq-node arc, slice 25)"
parent: Reference
---

# Round 743 dmq-node ocert-counter validation (dmq-node arc, slice 25)

Date: 2026-05-21

## Scope

Slice 25 of the dmq-node arc — the operational-certificate counter
monotonicity check, the first stateful `validate_sig` check over
`PoolValidationCtx`.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `validate_ocert_counter` — verifies a DMQ signature's
  operational-certificate counter is monotonic for the issuing pool,
  recording the observed counter in the context's `ocert_map`. Mirror
  of upstream `validateSig`'s ocert-counter check (`Validate.hs`): an
  absent counter, or one not below the last seen value, is accepted
  and recorded; a counter below the last seen one fails with
  `InvalidOcertCounter { last_seen, received }`.

The function takes `&mut PoolValidationCtx` (R742) — it both reads
the last-seen counter and records the new one, exactly as upstream's
`Map.alterF` over `vctxOcertMap`.

2 unit tests: accept-and-record (first sighting + non-decreasing
update), and rejection of a regressing counter (the rejected value
must not overwrite the recorded one).

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 106 lib (+2 vs R742's 104) +
  2 golden, all green.

## Remaining (dmq-node arc)

- The remaining `validate_sig` checks — pool eligibility against the
  stake snapshots (with clock skew), and the cryptographic OCert /
  KES-signature verification.
- `Configuration/Topology.hs`; the client / server protocol drivers;
  the `NodeKernel` / `Diffusion/*` run-loop wiring; `Tracer.hs`.
