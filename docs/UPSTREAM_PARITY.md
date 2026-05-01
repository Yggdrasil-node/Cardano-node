---
title: Upstream Parity Matrix
layout: default
parent: Reference
nav_order: 5
---

# Upstream Parity Matrix

Last updated: 2026-05-01

This document tracks concrete parity alignment against official IntersectMBO repositories and highlights remaining gaps that block a full parity claim.

## Verification Baseline

- `cargo fmt --all -- --check`: clean (zero diff; CI gate added under audit M-2 follow-up)
- `cargo check-all`: passing
- `cargo test-all`: passing (0 failures; workspace coverage is 4.7K+ tests at R238)
- `cargo lint`: passing (clippy `-D warnings` clean across all crates and targets)
- `cargo deny check advisories bans licenses sources`: passing (one intentional ignore: `RUSTSEC-2021-0127` for the `serde_cbor` storage carve-out, tracked separately for migration)
- **`cardano-cli 10.16` LSQ parity** (R164–R238): all 11 always-available cardano-cli queries (`tip`, `protocol-parameters`, `era-history`, `slot-number`, `utxo --whole-utxo`/`--address`/`--tx-in`, `tx-mempool info`/`next-tx`/`tx-exists`, `submit-tx`) decode end-to-end against yggdrasil's NtC socket on preview, preprod, and mainnet. With opt-in `YGG_LSQ_ERA_FLOOR=6` the era-gated queries plus every Conway-era cardano-cli subcommand decode end-to-end. Tail-end Conway dispatcher tag 36 `GetPoolDistr2` serves live `PoolDistr` data from the `set` stake snapshot with optional pool filtering (R237); `GetStakeDistribution`/`GetStakeDistribution2`, `GetSPOStakeDistr`, and `LedgerPeerSnapshotV2` likewise use live stake snapshot data when available. R238 makes `protocol-state` use the exact ChainDepState sidecar for the acquired point/tip. **The Conway-era LSQ wire-protocol gap is fully closed** (R190) and the remaining LSQ work is now edge-case data validation rather than placeholder removal.

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
| Storage | ouroboros-consensus storage | Near parity | Immutable/volatile/checkpoint model implemented; R238 ChainDepState sidecars follow LedgerDB restore-and-replay semantics without changing checkpoint CBOR |
| Monitoring/tracing | cardano-node + cardano-tracer | Near parity | Forwarder backend wired; aggregate server egress and lifetime peer stats covered; ongoing interoperability/endurance validation |

## Open Gaps

- No critical parity blockers are currently tracked in this matrix.
- Active validation focus: systematic mainnet endurance rehearsal, parallel BlockFetch §6.5 sign-off, and restart resilience evidence before changing the default `max_concurrent_block_fetch_peers`.
- Fixture maintenance focus: refresh `cardano-base` upstream vectors and update the pinned SHA metadata.

## Upstream Anchors

- cardano-node: https://github.com/IntersectMBO/cardano-node
- cardano-ledger: https://github.com/IntersectMBO/cardano-ledger
- ouroboros-consensus: https://github.com/IntersectMBO/ouroboros-consensus
- LedgerDB/openDB restore and replay semantics: https://ouroboros-consensus.cardano.intersectmbo.org/haddocks/ouroboros-consensus/Ouroboros-Consensus-Storage-LedgerDB.html
- Caught-up node storage model: https://ouroboros-consensus.cardano.intersectmbo.org/docs/explanations/node_tasks/
- UTxO-HD rollback/snapshot design: https://ouroboros-consensus.cardano.intersectmbo.org/docs/references/miscellaneous/utxo-hd/utxo-hd_in_depth/
- ouroboros-network: https://github.com/IntersectMBO/ouroboros-network
- cardano-base: https://github.com/IntersectMBO/cardano-base
- plutus: https://github.com/IntersectMBO/plutus

## Pinned commits (audit baseline 2026-Q2)

Yggdrasil is a pure-Rust port; there are no Cargo `git =` dependencies, so pinning is documentary. Each SHA below records the exact upstream commit at which the corresponding repository was last systematically audited against. The companion drift detector at `node/scripts/check_upstream_drift.sh` produces a JSON report comparing each pin to the live HEAD of the matching `main`/`master` branch.

| Repository | Pinned commit | Source |
|---|---|---|
| `cardano-base` | `db52f43b38ba5d8927feb2199d4913fe6c0f974d` | `node/src/upstream_pins.rs` (mirrors vendored `specs/upstream-test-vectors/cardano-base/<sha>/` directory name; pin advance gated on vendored fixture refresh) |
| `cardano-ledger` | `42d088ed84b799d6d980f9be6f14ad953a3c957d` | `node/src/upstream_pins.rs` (audit baseline 2026-04-30, R201 advance) |
| `ouroboros-consensus` | `b047aca4a731d3282b1dab012d3669e9395328cc` | `node/src/upstream_pins.rs` (audit baseline 2026-04-30, R216 advance) |
| `ouroboros-network` | `0e84bced45c7fc64252d576fbce55864d75e722a` | `node/src/upstream_pins.rs` (audit baseline 2026-04-26, in-sync with HEAD) |
| `plutus` | `4cd40a14e36431019414fad519c1a6d426a55509` | `node/src/upstream_pins.rs` (audit baseline 2026-04-30, R216 advance) |
| `cardano-node` | `799325937a4598899c8cab61f4c957662a0aeb53` | `node/src/upstream_pins.rs` (audit baseline 2026-04-30, R201 advance) |

**Format invariants** (drift-guarded by `crates/node::upstream_pins::tests`):

- All SHAs are 40-character lowercase hexadecimal.
- The set of pinned repositories is exactly the 6 listed above; adding/removing requires updating both `UPSTREAM_PINS` in source and this table.
- The `cardano-base` pin must match the vendored test-vector directory name in `specs/upstream-test-vectors/cardano-base/<sha>/`.

**To advance a pin**: edit `node/src/upstream_pins.rs`, run the full audit cadence (`cargo check-all`, `cargo test-all`, `cargo lint`, drift-guard tests, fixture cross-checks) against the new SHA, run `node/scripts/check_upstream_drift.sh` to confirm, then update this table with the rationale.

**Drift is expected and informational**. The audit baseline is allowed to lag upstream — the drift report exists so the lag is visible, not so it triggers a build failure. `check_upstream_drift.sh` exits 0 on drift by default; pass `--fail-on-drift` for CI gating if/when desired.

### Drift snapshot — 2026-04-30 (post-R216 advance)

`node/scripts/check_upstream_drift.sh` against live `git ls-remote HEAD`:

| Repository | Pinned (audit baseline) | Live HEAD (2026-04-30) | Status |
|---|---|---|---|
| `cardano-base` | `db52f43b38ba…` | `7a8a991945d4…` | drifted (vendored-fixture coupled — see below) |
| `cardano-ledger` | `42d088ed84b7…` | (same) | **in-sync** |
| `ouroboros-consensus` | `b047aca4a731…` | (same) | **in-sync** (R216 advance) |
| `ouroboros-network` | `0e84bced45c7…` | (same) | **in-sync** |
| `plutus` | `4cd40a14e364…` | (same) | **in-sync** (R216 advance) |
| `cardano-node` | `799325937a45…` | (same) | **in-sync** |

R201 (2026-04-30) advanced 4 of the 5 drifted documentary pins to live HEAD.  R216 (2026-04-30) refreshed the two pins that had drifted again since R201 (`ouroboros-consensus` and `plutus`).  All 5 documentary pins (every non-cardano-base pin) are in-sync at this point.  `cardano-base` remains at the original 2026-Q2 baseline because its SHA is mirrored by the vendored test-vector directory name (`specs/upstream-test-vectors/cardano-base/<sha>/`) consumed by `crates/crypto/tests/upstream_vectors.rs::CARDANO_BASE_SHA`; advancing it requires a coordinated refresh of the vendored fixtures and re-running the full corpus drift-guard tests, which is intentionally a separate audit slice (Phase E.1 cardano-base item).

The Yggdrasil code surface remains tested against the audit-baseline behavior; the documentary pins record which upstream commit the audit cadence (`cargo check-all`, `cargo test-all`, `cargo lint`, drift-guard tests) was last run against.

## Update Rules

- Update this file whenever a parity milestone or blocker status changes.
- Keep claims evidence-based: include command output or test references for any status changes.
- Keep this file synchronized with `docs/PARITY_PLAN.md`, `docs/PARITY_SUMMARY.md`, `docs/ARCHITECTURE.md`, and `README.md`.
