---
title: "Round 591 Remove inert ntn feature flag (A1 partial)"
parent: Reference
---

# Round 591 Remove inert ntn feature flag (A1 partial)

Date: 2026-05-20

## Scope

This round advances Category A1 (feature-flag gating) by removing
the inert `ntn` (node-to-node) feature flag from
`yggdrasil-network`.

Pivots away from the tx-generator R-arc (which has reached the
operator-soak gate after R590 closed every documented byte-parity
gap) to a fresh Category A item that's executable in this
environment.

## Justification

The `ntn` flag had been decorative since Wave 3 PR 5:

- 0 `#[cfg(feature = "ntn")]` gates anywhere in
  `yggdrasil-network/src/` or `crates/node/`.
- 0 downstream Cargo.toml entries opting in or out (no
  `features = ["ntn"]` or `default-features = false` clauses
  paired with an `ntn` opt-out across the workspace).
- Every yggdrasil consumer (relays, block producers, sister tools
  with node-to-node connectivity) requires node-to-node
  mini-protocols. The COMPLETION_ROADMAP A1 narrative explicitly
  identified `ntn` as a removal candidate rather than a wiring
  candidate.

Gating node-to-node mini-protocols behind a flag would have
required gating the bulk of `yggdrasil-network/src/protocols/` —
substantial refactor with no operational benefit.

## Changes

- `crates/network/Cargo.toml`:
  - `default = ["ntn", "ntc"]` → `default = ["ntc"]`
  - Removed `ntn = []` from `[features]`.
  - Replaced the Wave-3-PR-5 comment block with a current
    summary explaining the R591 removal and noting that `ntc`
    remains as the next wireable candidate.
- `docs/COMPLETION_ROADMAP.md` A1 section updated to reflect the
  removal and re-scope the remaining flag list to `ntc` +
  `plutus`.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-network` (886 lib tests + 4 doctests +
  139 integration tests — no regressions)

## Remaining (A1)

- `yggdrasil-network/ntc`: cleanly wireable — multi-crate round
  (gates the NtC mini-protocol module tree + the
  `yggdrasil-node-ntc-server` crate + the binary's `query` and
  `submit-tx` subcommands).
- `yggdrasil-ledger/plutus`: gates Alonzo+ phase-2 witness paths
  across ~8 per-era ledger apply-rule files; needs a slim-build
  soundness decision (a node without it skips phase-2 validation).
