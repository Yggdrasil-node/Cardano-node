---
title: "Round 741 dmq-node Policy (dmq-node arc, slice 23)"
parent: Reference
---

# Round 741 dmq-node Policy (dmq-node arc, slice 23)

Date: 2026-05-21

## Scope

Slice 23 of the dmq-node arc — the `SigSubmission` decision policy
and ingress limit (strict mirror of `DMQ/Policy.hs`).

## What shipped

`crates/tools/dmq-node/src/policy.rs` — new file, strict mirror of
`DMQ/Policy.hs`:

- `MAX_SIG_SIZE` (2800) / `MAX_SIGS_INFLIGHT` (33) constants.
- `SigDecisionPolicy` + `sig_decision_policy()` — mirror of upstream
  `TxDecisionPolicy` / `sigDecisionPolicy`:
  `maxUnacknowledgedTxIds` = `4 * maxSigsInflight` (132),
  `txsSizeInflightPerPeer` = `maxSigSize * maxSigsInflight` (92 400),
  `scoreMax` = `15 * 60` (900 s), `scoreRate` 0.1.
- `MiniProtocolLimits` + `sig_submission_ingress_limit()` — mirror of
  upstream `sigSubmissionIngressLimit`: the `addMargin`
  (`x + x / 10`, +10%) of `txsSizeInflightPerPeer` — 101 640 bytes.

`TxDecisionPolicy` and `MiniProtocolLimits` are network / mux types
upstream; `crates/network` does not expose them by those names, so
dmq-node carries its own mirrors (the R731 / R732 dmq-node-local
pattern). `lib.rs` gains `pub mod policy;`.

2 unit tests covering the decision-policy values and the
ingress-limit margin.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the new file).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 102 lib (+2 vs R740's 100) +
  2 golden, all green.

## Remaining (dmq-node arc)

- The client / server protocol drivers; `Configuration/Topology.hs`;
  the `Diffusion/*` run-loop wiring; `Tracer.hs`.
