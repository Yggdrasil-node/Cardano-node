---
title: "Round 746 dmq-node cryptographic validation (dmq-node arc, slice 28)"
parent: Reference
---

# Round 746 dmq-node cryptographic validation (dmq-node arc, slice 28)

Date: 2026-05-21

## Scope

Slice 28 of the dmq-node arc — the cryptographic `validate_sig`
checks, completing the `SigSubmission` validator's check set.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `validate_ocert_signature` — verifies the operational
  certificate's cold-key signature. Mirror of upstream `validateSig`'s
  `validateOCert` check; delegates to the consensus
  `OpCert::verify` (the Ed25519 signature over the certificate's
  signable representation). A failure maps to `InvalidSignatureOcert`.
- `validate_kes_signature` — verifies the KES signature over the
  signed payload bytes. Mirror of upstream's `verifyKES () ocertVkHot
  (sigKESPeriod - startKESPeriod) signedBytes kesSig`; the evolution
  is `sig_kes_period - ocert_kes_period`. Delegates to
  `yggdrasil_crypto::verify_sum_kes`; a failure maps to
  `InvalidKesSignature`.

2 unit tests verify the error-mapping (a garbage operational
certificate / KES signature is rejected with the typed error).

## SigSubmission validator — check set complete

The `SigSubmission` validator now has every `validateSig` check as a
standalone, parity-cited function: `validate_kes_period` (R727),
`validate_ocert_counter` (R743), `validate_pool_eligibility` (R744),
and `validate_ocert_signature` / `validate_kes_signature` (this
round). A later slice composes them into the full `validate_sig`
batch entry point.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 112 lib (+2 vs R745's 110) +
  2 golden, all green.

## Remaining (dmq-node arc)

- The `validate_sig` batch entry point composing the five checks.
- `Configuration/Topology.hs`; the client / server protocol drivers;
  the `NodeKernel` / `Diffusion/*` run-loop wiring; `Tracer.hs`.
