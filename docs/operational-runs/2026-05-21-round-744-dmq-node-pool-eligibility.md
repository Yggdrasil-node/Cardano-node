---
title: "Round 744 dmq-node pool-eligibility validation (dmq-node arc, slice 26)"
parent: Reference
---

# Round 744 dmq-node pool-eligibility validation (dmq-node arc, slice 26)

Date: 2026-05-21

## Scope

Slice 26 of the dmq-node arc — the pool-eligibility `validate_sig`
check (the stake-snapshot branching with the clock-skew window).

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `validate_pool_eligibility` — verifies a DMQ signature's issuing
  pool is registered and eligible to mint. Mirror of upstream
  `validateSig`'s pool-eligibility check (`Validate.hs`):
  - An unknown pool fails with `NotInitialized` (no epoch yet) or
    `UnrecognizedPool`.
  - `NotZeroSetSnapshot` (set stake non-zero): eligible while `now`
    is within `MAX_CLOCK_SKEW_SEC` of the next epoch boundary;
    otherwise `SigExpired` (mark stake zero) or `ClockSkew`.
  - `NotZeroMarkSnapshot` (set zero, mark non-zero): eligible within
    `±MAX_CLOCK_SKEW_SEC` of the boundary; otherwise `PoolNotEligible`
    (boundary still ahead) or `ClockSkew`.
  - `ZeroSetSnapshot` (set and mark both zero): `SigExpired`.

`UTCTime` is modelled as `u64` POSIX seconds; the signed
`diffUTCTime` arithmetic uses `i64`.

3 unit tests covering the unknown-pool, set-snapshot, and
mark-snapshot / zero-snapshot branches.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 109 lib (+3 vs R743's 106) +
  2 golden, all green.

## Remaining (dmq-node arc)

- The cryptographic `validate_sig` checks — OCert signature
  verification and KES-signature verification of the payload.
- `Configuration/Topology.hs`; the client / server protocol drivers;
  the `NodeKernel` / `Diffusion/*` run-loop wiring; `Tracer.hs`.
