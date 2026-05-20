---
title: "Round 592 Remove inert plutus feature flag from yggdrasil-ledger (A1 partial)"
parent: Reference
---

# Round 592 Remove inert plutus feature flag from yggdrasil-ledger (A1 partial)

Date: 2026-05-20

## Scope

This round continues Category A1 (feature-flag gating) by removing
the inert `plutus` feature flag from `yggdrasil-ledger`. Mirrors
R591's `ntn` removal pattern.

## Justification

The Phase-5.4 audit comment in `crates/ledger/Cargo.toml` (from
2026-05) had already concluded that wiring this flag was mis-scoped:

- 0 `#[cfg(feature = "plutus")]` gates anywhere in
  `crates/ledger/src/`.
- 0 downstream Cargo.toml entries opting in or out.
- `crates/ledger` does NOT depend on `crates/plutus`. The heavy
  CEK-machine / cost-model code lives behind the inverted
  `PlutusEvaluator` trait, wired by `crates/node/plutus-eval` and
  the node binary, not by the ledger crate.
- `crates/ledger`'s own phase-2 code (`plutus_validation.rs`) is
  ~1.4 KLoC of pure trait-orchestration with no heavy dependency
  tree, and `validate_plutus_scripts` already takes
  `Option<&dyn PlutusEvaluator>` — the evaluator is already
  optional at the call boundary.
- Gating `plutus_validation` off would not slim the dependency
  graph or the binary; it would only remove validation logic —
  a correctness hazard, not a build-slimming win.

Genuine Plutus build-slimming belongs at the
`crates/node/plutus-eval` bridge / node-binary level (gate
`crates/plutus` itself out of the graph), not on this crate's
feature flag.

## Changes

- `crates/ledger/Cargo.toml`:
  - Removed `[features] default = ["plutus"]` and `plutus = []`.
  - Updated the comment block to record the R592 removal and
    explain why genuine Plutus build-slimming belongs elsewhere.
- `docs/COMPLETION_ROADMAP.md` A1 section updated: only
  `yggdrasil-network/ntc` remains as a wireable inert flag.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-ledger` (1181 lib tests + 756 integration
  + 7 doctests — no regressions)

## Remaining (A1)

- `yggdrasil-network/ntc`: cleanly wireable — multi-crate round
  (gates the NtC mini-protocol module tree + the
  `yggdrasil-node-ntc-server` crate + the binary's `query` and
  `submit-tx` subcommands).
