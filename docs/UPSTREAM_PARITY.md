# Upstream Parity Matrix

Last updated: 2026-04-17

This document tracks concrete parity alignment against official IntersectMBO repositories and highlights remaining gaps that block a full parity claim.

## Verification Baseline

- `cargo check-all`: passing
- `cargo test-all`: passing (0 failures)
- `cargo test-all -- --list`: 4137 discovered tests
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
| Node orchestration | cardano-node | Partial parity | Runtime orchestration complete for main paths; some bridge items remain |
| Storage | ouroboros-consensus storage | Near parity | Immutable/volatile/checkpoint model implemented |
| Monitoring/tracing | cardano-node + cardano-tracer | Partial parity | Forwarder backend wired; ongoing interoperability/endurance validation |

## Open Gaps

1. Live consensus-network ledger peer bridge:
- Startup and reconnect paths are implemented, but the architecture still marks the consensus-fed live bridge as in progress.
- Complete replacement of node-owned orchestration with a fully live consensus-fed path remains open.

## Upstream Anchors

- cardano-node: https://github.com/IntersectMBO/cardano-node
- cardano-ledger: https://github.com/IntersectMBO/cardano-ledger
- ouroboros-consensus: https://github.com/IntersectMBO/ouroboros-consensus
- ouroboros-network: https://github.com/IntersectMBO/ouroboros-network
- cardano-base: https://github.com/IntersectMBO/cardano-base
- plutus: https://github.com/IntersectMBO/plutus

## Update Rules

- Update this file whenever a parity milestone or blocker status changes.
- Keep claims evidence-based: include command output or test references for any status changes.
- Keep this file synchronized with `docs/PARITY_PLAN.md`, `docs/PARITY_SUMMARY.md`, `docs/ARCHITECTURE.md`, and `README.md`.
