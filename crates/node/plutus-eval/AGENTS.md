# yggdrasil-node-plutus-eval — Plutus script evaluator wrapper

## Scope

Wraps `yggdrasil-plutus::CekMachine` for the node runtime's
phase-2 transaction validation. Extracted from `yggdrasil-node`
in Wave 5 PR 10 so the `default = ["plutus"]` feature flag
on `yggdrasil-node` actually slims the binary when disabled
(a "no-Plutus relay" build for header-only validators).

The crate ships `CekPlutusEvaluator`, the time-conversion + cost-
model-resolution glue that wraps the pure `CekMachine` so it has
access to per-block slot→posix-ms conversion and per-era cost-model
selection.

## Rules — Non-Negotiable

- **Leaf in the build graph below `yggdrasil-node-runtime`.** Depends
  only on `yggdrasil-{ledger,plutus}` and `yggdrasil-node-genesis`.
  Adding any other node-sub-crate dep re-introduces the coupling
  Wave 5 broke.
- **Feature-flag conscious.** The `yggdrasil-node` binary's `plutus`
  feature controls whether this crate is linked at all. Adding
  unconditional consumers in sister crates breaks the slim build.

## Naming parity

Synthesis crate. The lib.rs (former node/src/plutus_eval.rs) carries
the `## Naming parity` stanza.

## R-arc tracking

Wave 5 PR 10. Wave 6 PR 17 (R502) does not directly touch this crate;
the cardano-tracer integration is on `yggdrasil-node-tracer`.
