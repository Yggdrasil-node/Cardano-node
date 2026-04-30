---
title: Upstream Parity Matrix
layout: default
parent: Reference
nav_order: 5
---

# Upstream Parity Matrix

Last updated: 2026-04-27

This document tracks concrete parity alignment against official IntersectMBO repositories and highlights remaining gaps that block a full parity claim.

## Verification Baseline

- `cargo fmt --all -- --check`: clean (zero diff; CI gate added under audit M-2 follow-up)
- `cargo check-all`: passing
- `cargo test-all`: passing (0 failures, **4 744** discovered tests, 1 ignored — count current at R190)
- `cargo lint`: passing (clippy `-D warnings` clean across all crates and targets)
- `cargo deny check advisories bans licenses sources`: passing (one intentional ignore: `RUSTSEC-2021-0127` for the `serde_cbor` storage carve-out, tracked separately for migration)
- **`cardano-cli 10.16` LSQ parity** (R164–R190): all 11 always-available cardano-cli queries (`tip`, `protocol-parameters`, `era-history`, `slot-number`, `utxo --whole-utxo`/`--address`/`--tx-in`, `tx-mempool info`/`next-tx`/`tx-exists`, `submit-tx`) decode end-to-end against yggdrasil's NtC socket on preprod (Shelley) and preview (Alonzo).  With opt-in `YGG_LSQ_ERA_FLOOR=6` the era-gated queries (`stake-pools`, `stake-distribution`, `pool-state`, `stake-snapshot`, `stake-address-info`, `ref-script-size`) plus **every Conway-era subcommand** (`constitution`, `gov-state`, `drep-state`, `drep-stake-distribution`, `committee-state`, `treasury`, `spo-stake-distribution`, `proposals`, `ratify-state`, `future-pparams`, `stake-pool-default-vote`, `ledger-peer-snapshot`, `protocol-state`, `ledger-state`) decode end-to-end.  Tail-end Conway dispatchers (tags 22 `GetStakeDelegDeposits`, 36 `GetPoolDistr2`) are wire-correct (R186) — no direct cardano-cli subcommands but emit valid empty placeholders.  **The Conway-era LSQ wire-protocol gap is fully closed** (R190 comprehensive audit confirmed 28 cardano-cli subcommands working end-to-end).  Remaining work is data-plumbing for the placeholder fields rather than wire-shape parity.

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
- **Conway governance LSQ — `gov-state` body shape** (R180 dispatcher routes; 7-element `ConwayGovState` record with `Proposals` tree + `DRepPulsingState` cache pending).
- **Live stake-snapshot plumbing** (R163/R173/R179 follow-up): `query stake-distribution` and `query stake-snapshot` currently emit 1-lovelace `NonZero Coin` placeholders for totals; routing the runtime's `mark`/`set`/`go` snapshot rotation into the LSQ `LedgerStateSnapshot` would surface real per-pool stake.
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
| `cardano-base` | `db52f43b38ba5d8927feb2199d4913fe6c0f974d` | `node/src/upstream_pins.rs` (mirrors vendored `specs/upstream-test-vectors/cardano-base/<sha>/` directory name; pin advance gated on vendored fixture refresh) |
| `cardano-ledger` | `42d088ed84b799d6d980f9be6f14ad953a3c957d` | `node/src/upstream_pins.rs` (audit baseline 2026-04-30, R201 advance) |
| `ouroboros-consensus` | `c368c2529f2f41196461883013f749b7ac7aa58e` | `node/src/upstream_pins.rs` (audit baseline 2026-04-30, R201 advance) |
| `ouroboros-network` | `0e84bced45c7fc64252d576fbce55864d75e722a` | `node/src/upstream_pins.rs` (audit baseline 2026-04-26, in-sync with HEAD) |
| `plutus` | `e3eb4c76ea20cf4f90231a25bdfaab998346b406` | `node/src/upstream_pins.rs` (audit baseline 2026-04-30, R201 advance) |
| `cardano-node` | `799325937a4598899c8cab61f4c957662a0aeb53` | `node/src/upstream_pins.rs` (audit baseline 2026-04-30, R201 advance) |

**Format invariants** (drift-guarded by `crates/node::upstream_pins::tests`):

- All SHAs are 40-character lowercase hexadecimal.
- The set of pinned repositories is exactly the 6 listed above; adding/removing requires updating both `UPSTREAM_PINS` in source and this table.
- The `cardano-base` pin must match the vendored test-vector directory name in `specs/upstream-test-vectors/cardano-base/<sha>/`.

**To advance a pin**: edit `node/src/upstream_pins.rs`, run the full audit cadence (`cargo check-all`, `cargo test-all`, `cargo lint`, drift-guard tests, fixture cross-checks) against the new SHA, run `node/scripts/check_upstream_drift.sh` to confirm, then update this table with the rationale.

**Drift is expected and informational**. The audit baseline is allowed to lag upstream — the drift report exists so the lag is visible, not so it triggers a build failure. `check_upstream_drift.sh` exits 0 on drift by default; pass `--fail-on-drift` for CI gating if/when desired.

### Drift snapshot — 2026-04-30 (post-R201 advance)

`node/scripts/check_upstream_drift.sh` against live `git ls-remote HEAD`:

| Repository | Pinned (audit baseline) | Live HEAD (2026-04-30) | Status |
|---|---|---|---|
| `cardano-base` | `db52f43b38ba…` | `9965336f769d…` | drifted (vendored-fixture coupled — see below) |
| `cardano-ledger` | `42d088ed84b7…` | (same) | **in-sync** |
| `ouroboros-consensus` | `c368c2529f2f…` | (same) | **in-sync** |
| `ouroboros-network` | `0e84bced45c7…` | (same) | **in-sync** |
| `plutus` | `e3eb4c76ea20…` | (same) | **in-sync** |
| `cardano-node` | `799325937a45…` | (same) | **in-sync** |

R201 (2026-04-30) advanced 4 of the 5 drifted documentary pins to live HEAD: `cardano-ledger`, `ouroboros-consensus`, `plutus`, `cardano-node`.  Five of six pins are now in-sync.  `cardano-base` remains pinned at the original 2026-Q2 baseline because its SHA is mirrored by the vendored test-vector directory name (`specs/upstream-test-vectors/cardano-base/<sha>/`) consumed by `crates/crypto/tests/upstream_vectors.rs::CARDANO_BASE_SHA`; advancing it requires a coordinated refresh of the vendored fixtures and re-running the full corpus drift-guard tests, which is intentionally a separate audit slice.

The Yggdrasil code surface remains tested against the audit-baseline behavior; the documentary pins record which upstream commit the audit cadence (`cargo check-all`, `cargo test-all`, `cargo lint`, drift-guard tests) was last run against.

## Update Rules

- Update this file whenever a parity milestone or blocker status changes.
- Keep claims evidence-based: include command output or test references for any status changes.
- Keep this file synchronized with `docs/PARITY_PLAN.md`, `docs/PARITY_SUMMARY.md`, `docs/ARCHITECTURE.md`, and `README.md`.
