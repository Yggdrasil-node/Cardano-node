---
title: Upstream Parity Matrix
layout: default
parent: Reference
nav_order: 5
---

# Upstream Parity Matrix

Last updated: 2026-05-02

This document tracks concrete parity alignment against official IntersectMBO repositories and highlights remaining gaps that block a full parity claim.

## Verification Baseline

- `cargo fmt --all -- --check`: passing after the latest R248 patches
- Focused R246 Plutus/ledger/node tests and `cargo build -p yggdrasil-node --release`: passing
- Focused R247 Origin-prefix BlockFetch test plus bounded clean preview replay to slot `101100`: passing
- Focused R248 TPraos overlay tests plus live preview resume to Babbage slot `868687`: passing
- `cargo check-all`: passing after the latest R248 patches
- `cargo test-all`: passing after the latest R248 patches
- `cargo lint`: passing after the latest R248 patches
- `cargo deny check advisories bans licenses sources`: passing (one intentional ignore: `RUSTSEC-2021-0127` for the `serde_cbor` storage carve-out, tracked separately for migration)
- **Genesis preflight parity** (R244): all four configured genesis hashes are verified at startup. Byron follows upstream `Cardano.Chain.Genesis.Data.readGenesisData` by hashing `renderCanonicalJSON` after `parseCanonicalJSON`; Shelley, Alonzo, and Conway continue to use raw-file Blake2b-256.
- **Conway BBODY drift parity** (R245): `HeaderProtVerTooHigh` now follows upstream's `netId == Mainnet || curProtVerMajor >= 12` condition, so mainnet remains strict, pre-Dijkstra testnets get the temporary grace path, and testnets re-enable the check at Dijkstra protocol major 12. The separate `MaxMajorProtVer` cap remains enforced on every network.
- **Preview Plutus replay parity** (R246): well-formedness remains enforced while on-chain scripts are treated as raw `PlutusBinary` bytes (CBOR bytestring containing Flat) under protocol-version language gates; Babbage/Conway reference inputs are ordered by `ShelleyTxIn`; CEK non-constant runtime values use `ExMemory = 1`; Plutus `Integer` values use arbitrary precision across Flat, CBOR `PlutusData`, and builtins; pre-Conway upper-only validity intervals use inclusive `PV1.to`; Plutus `serialiseData` uses upstream CBOR shape; and legacy `AccountRegistration` is not over-collected as a Certifying purpose. Release refscan reached preview slot `901725` with no `MalformedReferenceScripts`, `ValidationTagMismatch`, ledger decode error, or missing legacy-registration redeemer. A later bounded live run reached checkpoint slot `1038614` before exposing stale persisted reward state from a pre-fix runtime recovery path, not a Plutus failure.
- **Preview Origin-prefix BlockFetch parity** (R247): a verified sync batch that starts at `Point::Origin` but collects multiple ChainSync roll-forward headers now uses the first announced concrete header as the BlockFetch lower bound. This preserves the slot-0 preview prefix instead of fetching only the final announced header; a clean replay verified slots `0`, `60`, `300`, and `320` are present and advanced to slot `101100` without the prior missing-UTxO failure.
- **Preview TPraos overlay VRF parity** (R248): Shelley-family TPraos active overlay slots now classify the epoch overlay schedule and apply the upstream genesis-delegate branch: verify the selected genesis delegate cold key, the delegate VRF key, and both TPraos VRF proofs, then skip the pool stake leader-threshold check. Reserved non-active overlay slots fail closed. A live preview resume passed the former active-overlay blocker at slot `106220`, crossed the prior `730728`/`840719` Plutus stops, and advanced to Babbage slot `868687` with no `VRF verification failed`, `MalformedReferenceScripts`, `ValidationTagMismatch`, ledger decode error, or panic.
- **`cardano-cli 10.16` LSQ parity** (R164–R240): all 11 always-available cardano-cli queries (`tip`, `protocol-parameters`, `era-history`, `slot-number`, `utxo --whole-utxo`/`--address`/`--tx-in`, `tx-mempool info`/`next-tx`/`tx-exists`, `submit-tx`) decode end-to-end against yggdrasil's NtC socket on preview, preprod, and mainnet. With opt-in `YGG_LSQ_ERA_FLOOR=6` the era-gated queries plus every Conway-era cardano-cli subcommand decode end-to-end, including `conway query gov-state` (R188/R193/R204). Tail-end Conway dispatcher tag 36 `GetPoolDistr2` serves live `PoolDistr` data from the `set` stake snapshot with optional pool filtering (R237); `GetStakeDistribution`/`GetStakeDistribution2`, `GetSPOStakeDistr`, and `LedgerPeerSnapshotV2` likewise use live stake snapshot data when available. R238 makes `protocol-state` use the exact ChainDepState sidecar for the acquired point/tip, R239 closes the coordinated `cardano-base` fixture refresh, R240 adds reproducible §6.5 BlockFetch soak automation, and R245 refreshes the latest `cardano-ledger` documentary pin. **The Conway-era LSQ wire-protocol gap is fully closed** (R190) and the remaining LSQ work is now edge-case data validation rather than placeholder removal.

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

- No critical code-level Plutus parity blocker is currently tracked in this matrix.
- Active validation focus: run clean/repaired preview replay from the R248 overlay fix through the former stale-checkpoint reward-state stop; complete systematic mainnet endurance rehearsal plus runbook §6.5 sign-off using `node/scripts/parallel_blockfetch_soak.sh` before changing the default `max_concurrent_block_fetch_peers`.
- Fixture and Plutus maintenance focus: keep the R239 `cardano-base` vector cadence current when upstream advances again, and keep the R246 Plutus parity assumptions under replay/drift watch as new preview/preprod scripts appear.

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
| `cardano-base` | `7a8a991945d401d89e27f53b3d3bb464a354ad4c` | `node/src/upstream_pins.rs` (R239 fixture refresh; mirrors vendored `specs/upstream-test-vectors/cardano-base/<sha>/` directory name) |
| `cardano-ledger` | `b90b97488da3cbdc01c5c4a610c674a22d467882` | `node/src/upstream_pins.rs` (R245 BBODY/GOV refresh; mirrors upstream testnet `HeaderProtVerTooHigh` grace and accumulated-proposal hard-fork consistency cleanup) |
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

### Drift snapshot — 2026-05-01 (post-R245 cardano-ledger BBODY/GOV refresh)

`node/scripts/check_upstream_drift.sh` against live `git ls-remote HEAD`:

| Repository | Pinned (audit baseline) | Live HEAD (2026-05-01) | Status |
|---|---|---|---|
| `cardano-base` | `7a8a991945d4…` | (same) | **in-sync** (R239 fixture refresh) |
| `cardano-ledger` | `b90b97488da3…` | (same) | **in-sync** (R245 BBODY/GOV refresh) |
| `ouroboros-consensus` | `b047aca4a731…` | (same) | **in-sync** (R216 advance) |
| `ouroboros-network` | `0e84bced45c7…` | (same) | **in-sync** |
| `plutus` | `4cd40a14e364…` | (same) | **in-sync** (R216 advance) |
| `cardano-node` | `799325937a45…` | (same) | **in-sync** |

R201 (2026-04-30) advanced 4 of the 5 drifted documentary pins to live HEAD. R216 (2026-04-30) refreshed the two pins that had drifted again since R201 (`ouroboros-consensus` and `plutus`). R239 (2026-05-01) refreshed the coordinated `cardano-base` vendored fixture tree and advanced the mirrored `CARDANO_BASE_SHA` / `UPSTREAM_CARDANO_BASE_COMMIT` pins. R243 (2026-05-01) refreshed `cardano-ledger` after upstream PR #5787 removed a redundant import in `Cardano.Ledger.Shelley.API.Mempool`. R245 (2026-05-01) refreshed `cardano-ledger` again after upstream changed Conway GOV `preceedingHardFork` to accumulated proposals and temporarily suppressed BBODY `HeaderProtVerTooHigh` on testnets until Dijkstra; Yggdrasil mirrors the BBODY condition and already matched the accumulated-proposal GOV behavior. All 6 canonical upstream pins are now in-sync with live HEAD.

The Yggdrasil code surface remains tested against the audit-baseline behavior; the documentary pins record which upstream commit the audit cadence (`cargo check-all`, `cargo test-all`, `cargo lint`, drift-guard tests) was last run against.

## Update Rules

- Update this file whenever a parity milestone or blocker status changes.
- Keep claims evidence-based: include command output or test references for any status changes.
- Keep this file synchronized with `docs/PARITY_PLAN.md`, `docs/PARITY_SUMMARY.md`, `docs/ARCHITECTURE.md`, and `README.md`.
