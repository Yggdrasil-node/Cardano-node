---
title: Upstream Parity Matrix
layout: default
parent: Reference
nav_order: 5
---

# Upstream Parity Matrix

Last updated: 2026-04-26

This document tracks concrete parity alignment against official IntersectMBO repositories and highlights remaining gaps that block a full parity claim.

## Verification Baseline

- `cargo check-all`: passing
- `cargo test-all`: passing (0 failures)
- `cargo test-all -- --list`: 4210 discovered tests
- `cargo lint`: passing (clippy `-D warnings` clean across all crates and targets)

## Subsystem Status

| Subsystem | Upstream Reference | Status | Notes |
|---|---|---|---|
| Crypto | cardano-base | Near parity | Core primitives and vectors integrated; maintain vector refresh cadence |
| CDDL codegen | cardano-ledger CDDL + binary libs | Near parity | Generated type + codec workflow in place |
| Ledger rules | cardano-ledger | Near parity | Broad Conway/Shelley coverage implemented; continue edge-case parity audits |
| Consensus | ouroboros-consensus | Near parity | Praos/TPraos and chain-state core behavior implemented |
| Network protocols | ouroboros-network | Near parity | Handshake/mux/mini-protocol suites implemented |
| Peer governor | ouroboros-network diffusion/governor | Near parity | Policy engine implemented; keep behavior checks against upstream changes |
| Node orchestration | cardano-node | Near parity | Runtime orchestration complete for main paths, including the live consensus-fed ledger-peer bridge |
| Storage | ouroboros-consensus storage | Near parity | Immutable/volatile/checkpoint model implemented |
| Monitoring/tracing | cardano-node + cardano-tracer | Partial parity | Forwarder backend wired; ongoing interoperability/endurance validation |

## Open Gaps

- No critical parity blockers are currently tracked in this matrix.
- Active validation focus: systematic mainnet bring-up rehearsal (interop checkpoints, restart resilience, and endurance traces) to harden operational parity evidence.

## Upstream Anchors

- cardano-node: https://github.com/IntersectMBO/cardano-node
- cardano-ledger: https://github.com/IntersectMBO/cardano-ledger
- ouroboros-consensus: https://github.com/IntersectMBO/ouroboros-consensus
- ouroboros-network: https://github.com/IntersectMBO/ouroboros-network
- cardano-base: https://github.com/IntersectMBO/cardano-base
- plutus: https://github.com/IntersectMBO/plutus

## Pinned commits (audit baseline 2026-Q2)

Yggdrasil is a pure-Rust port; there are no Cargo `git =` dependencies, so pinning is documentary. Each SHA below records the exact upstream commit at which the corresponding repository was last systematically audited against. The companion drift detector at `node/scripts/check_upstream_drift.sh` produces a JSON report comparing each pin to the live HEAD of the matching `main`/`master` branch.

| Repository | Pinned commit | Source |
|---|---|---|
| `cardano-base` | `db52f43b38ba5d8927feb2199d4913fe6c0f974d` | `node/src/upstream_pins.rs` (mirrors vendored `specs/upstream-test-vectors/cardano-base/<sha>/` directory name) |
| `cardano-ledger` | `9ae77d611ad86ae58add04b6042ab730272f2327` | `node/src/upstream_pins.rs` (audit baseline 2026-04-26) |
| `ouroboros-consensus` | `91c8e1bb5d7fd9e1387755a0d539f8dce65737df` | `node/src/upstream_pins.rs` (audit baseline 2026-04-26) |
| `ouroboros-network` | `0e84bced45c7fc64252d576fbce55864d75e722a` | `node/src/upstream_pins.rs` (audit baseline 2026-04-26) |
| `plutus` | `187c3971a34e5ee4c42f4ea3b21eb61d1a7bad66` | `node/src/upstream_pins.rs` (audit baseline 2026-04-26) |
| `cardano-node` | `60af1c23bc20e64827574540599de1db1be2393e` | `node/src/upstream_pins.rs` (audit baseline 2026-04-26) |

**Format invariants** (drift-guarded by `crates/node::upstream_pins::tests`):

- All SHAs are 40-character lowercase hexadecimal.
- The set of pinned repositories is exactly the 6 listed above; adding/removing requires updating both `UPSTREAM_PINS` in source and this table.
- The `cardano-base` pin must match the vendored test-vector directory name in `specs/upstream-test-vectors/cardano-base/<sha>/`.

**To advance a pin**: edit `node/src/upstream_pins.rs`, run the full audit cadence (`cargo check-all`, `cargo test-all`, `cargo lint`, drift-guard tests, fixture cross-checks) against the new SHA, run `node/scripts/check_upstream_drift.sh` to confirm, then update this table with the rationale.

**Drift is expected and informational**. The audit baseline is allowed to lag upstream — the drift report exists so the lag is visible, not so it triggers a build failure. `check_upstream_drift.sh` exits 0 on drift by default; pass `--fail-on-drift` for CI gating if/when desired.

## Update Rules

- Update this file whenever a parity milestone or blocker status changes.
- Keep claims evidence-based: include command output or test references for any status changes.
- Keep this file synchronized with `docs/PARITY_PLAN.md`, `docs/PARITY_SUMMARY.md`, `docs/ARCHITECTURE.md`, and `README.md`.
