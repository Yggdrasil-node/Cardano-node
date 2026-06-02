---
title: "Round 742 dmq-node PoolValidationCtx (dmq-node arc, slice 24)"
parent: Reference
---

# Round 742 dmq-node PoolValidationCtx (dmq-node arc, slice 24)

Date: 2026-05-21

## Scope

Slice 24 of the dmq-node arc — the signature-validation context
types, opening the `diffusion` module.

## What shipped

`crates/tools/dmq-node/src/diffusion.rs` — new file. Ports the
self-contained validation-context data types from upstream
`DMQ/Diffusion/NodeKernel/Types.hs`:

- `PoolId` — the 28-byte pool key hash (upstream `KeyHash StakePool`).
- `StakeSnapshot` — the per-pool mark / set / go active stake (the
  minimal projection of upstream's ledger `StakeSnapshot` the
  `SigSubmission` validator needs).
- `PoolValidationCtx` — `epoch` (next-boundary POSIX time, for
  clock skew), `stake_map` (pool eligibility), `ocert_map`
  (operational-certificate counter monotonicity). `Default` is the
  not-yet-initialized state.

The runtime-heavy `NodeKernel` / `StakePools` records (STM vars,
fetch-client / peer-sharing registries) and the rest of
`Diffusion/*` land with the deferred Diffusion-wiring sub-arc.
`lib.rs` gains `pub mod diffusion;`.

This unblocks the stateful portion of `validate_sig` (pool
eligibility, opcert-counter monotonicity) — the validator slices
that R727 deferred pending `PoolValidationCtx`.

2 unit tests: the default uninitialized context, and the
stake / ocert maps.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the new file).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 104 lib (+2 vs R741's 102) +
  2 golden, all green.

## Remaining (dmq-node arc)

- The stateful `validate_sig` checks (pool eligibility, opcert-counter
  monotonicity, the cryptographic OCert / KES-signature checks) over
  `PoolValidationCtx`.
- `Configuration/Topology.hs`; the client / server protocol drivers;
  the `NodeKernel` / `Diffusion/*` run-loop wiring; `Tracer.hs`.
