---
title: "Round 727 dmq-node KES-period validation (dmq-node arc, slice 11)"
parent: Reference
---

# Round 727 dmq-node KES-period validation (dmq-node arc, slice 11)

Date: 2026-05-21

## Scope

Slice 11 of the dmq-node arc — opens the signature-validator port
(upstream `SigSubmission/Validate.hs`). Adds the `MAX_CLOCK_SKEW_SEC`
constant and the `validate_kes_period` check.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `MAX_CLOCK_SKEW_SEC` — `u64` 5; mirror of upstream
  `c_MAX_CLOCK_SKEW_SEC :: NominalDiffTime = 5`.
- `validate_kes_period` — verifies a signature's KES period lies in
  its operational certificate's validity window
  `[ocert_kes_period, ocert_kes_period + total_kes_periods)`. Mirror
  of upstream `validateSig`'s KES-period checks
  (`KESAfterEndOCERT` / `KESBeforeStartOCERT`), preserving the
  after-end-first check order. `total_kes_periods` (upstream's
  `totalPeriodsKES (Proxy (KES crypto))`) is a caller-supplied
  parameter — the full `validate_sig` will provide it.

A pure, dependency-free function — the first self-contained piece of
the validator. The rest of `validateSig` (pool-eligibility against
stake snapshots, opcert-counter monotonicity, the `validateOCert` /
KES-signature cryptographic checks) is stateful and entangled with
the `PoolValidationCtx` from `Diffusion/NodeKernel.hs`; it lands with
the Diffusion sub-arc.

2 unit tests: in-window acceptance (start inclusive, end exclusive)
and before-start / at-end / after-end rejection.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 70 lib (+2 vs R726's 68) +
  2 golden, all green.

## Remaining (dmq-node arc)

- The rest of `validateSig` — pool eligibility, opcert-counter
  monotonicity, `validateOCert` + KES-signature checks (needs the
  `PoolValidationCtx` from the Diffusion sub-arc).
- The `codecSigSubmission` TxSubmission2 wrapper + protocol-limit
  tables (a `yggdrasil-network` integration sub-arc).
- NodeToClient / NodeToNode protocols; Diffusion wiring.
