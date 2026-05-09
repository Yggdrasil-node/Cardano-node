---
title: Upstream Parity Matrix
layout: default
parent: Reference
nav_order: 5
---

# Upstream Parity Matrix

Last updated: 2026-05-05

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
- **R249 cumulative refresh** (2026-05-05): drift detector now reports all 6 documentary pins in-sync against live `master`/`main` HEAD (cardano-base, cardano-ledger, ouroboros-consensus, ouroboros-network, plutus, cardano-node). Per-repo audit confirmed the upstream changes since R245/R243/R239/R216/R201 are forward-looking only (Peras voting committees, Dijkstra `MemoBytes` `BlockBody`, post-Conway plutus `CInteger`/`CByteString` and cost models D/E, `StAnnTx` Haskell-internal threading, internal `submitTxToMempool` API change, cardano-testnet CLI restructure, experimental-hardfork PV12 bump) — no active-era CBOR codec, validation rule, transition-system semantic, or wire-protocol change. Companion fix: `local_server::tests::effective_era_index_pv_table_matches_upstream` and `era_floor_env_var_promotes_reported_era` now share a module-scope `ENV_LOCK` so the parallel test runner cannot leak `YGG_LSQ_ERA_FLOOR` between them and corrupt the PV→era_index table assertions.
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
- **Open consensus parity gap (R249, Gap BO)**: preprod TPraos VRF check fails at slot `~429460` in the Shelley `d=1` federation period. The R249 bounded soak with a freshly built binary synced from Origin through 4 epoch boundaries (all with `pparamUpdatesApplied=0`, so `decentralisationParam=1` from genesis stays in force) and persisted checkpoints up to slot `429460` before exiting with `consensus error: invalid VRF proof primaryPeer=3.79.79.217:3001`. Preview was already clean through Babbage slot `868687` (R248); preprod-specific divergence is the new finding, past R207's previous coverage at slot `87440`. Three plausible causes: (a) `lookup_tpraos_overlay_schedule` reports `Reserved` for a slot upstream considers `Active` (off-by-one in the genesis-delegate cycle), (b) the active-genesis-delegate VRF check itself fails on an active slot, (c) TPraos nonce evolution drift across the four epoch boundaries. Forensic log preserved at `docs/operational-runs/2026-05-05-round-249-preprod-vrf-failure-slot-429460.log`. Closes Yggdrasil's mainnet/preprod sync claim until resolved.
- **Open Plutus parity gap (R249, Gap BP)**: preview Plutus V2 budget overrun at slot `~1462057`. A 30-minute R249 preview sync from Origin advanced through 16 epoch boundaries — confirming **fees** (no `FeeTooSmall` across 1.46 M slots), **automatic pparam updates** (3 epochs landing 7 total real on-chain PPUP-applied updates), and **Alonzo→Babbage hardfork** all parity-correct — then failed at preview slot `~1462057` with a `Phase-2 validation tag mismatch` for a real V2 script. The block declared `is_valid=true` and budgeted `(mem=6_121_408, steps=1_657_962_006)`; Yggdrasil's CEK overran the CPU budget by `306_309` (≈ 0.0185 %) at a `200 accumulated steps` batched debit (`cost cpu=4_600_000, mem=20_000`). Magnitude points to per-`StepKind` CEK cost-model rounding vs. upstream `plutus-core/cost-model/data/builtinCostModelB.json`'s `cek*ConstCost`/`cekVarCost`/`cekLamCost`/`cekApplyCost`/`cekDelayCost`/`cekForceCost`/`cekBuiltinCost` fields, or step-kind miscount on a specific V2 term shape (`Constr`/`Case` are V3-only and not the culprit). Forensic log preserved at `docs/operational-runs/2026-05-05-round-249-preview-plutus-v2-budget-gap-slot-1462057.log`. Closes Yggdrasil's preview-tip parity claim until resolved. **Sync-only escape hatch (R249 same slice)**: `YGG_SKIP_PHASE2=1` causes `node/src/sync.rs::phase2_evaluator_or_trust_block` to pass `None` to `apply_block_validated`, so the ledger trusts the on-chain `is_valid` tag and skips Plutus Phase-2 re-execution for catch-up. Phase-1 (fees, witnesses, UTxO state, signatures, pparam updates, certificate processing, withdrawals) remains in force. Operators see a one-time `WARN: YGG_SKIP_PHASE2 is set` trace; **block-producing nodes must NEVER set this** since re-validation is the only way to catch CEK divergence before forging.
- **Closed consensus parity gap (R251, Gap BQ)**: preview VKey witness signature verification at slot `~1525024` on tx `44ccae438c4e1350271e772a96b4f974ee3a48c6458d7c2499a200abbdb55948`. With `YGG_SKIP_PHASE2=1` the R249 preview sync advanced past Gap BP through 17 epoch boundaries and reached slot `~1525024`, then failed with `VKey witness signature verification failed for hash 45d70e54f3b5e9c5a2b0cd417028197bd6f5fa5378c2f5eba896678d` from IOG bootstrap peer `99.80.240.19:3001`. **Initial hypothesis (libsodium-vs-`verify_strict` strictness divergence) was disproven** by direct byte-level inspection: signature R/S canonical, vkey not small-order, AND independent verification with OpenSSL Ed25519 (Python `cryptography`) ALSO rejected the signature against Yggdrasil's computed message — proving the bug was a wrong `tx_body_hash`, not a verifier-strictness issue. The security guardrail correctly stopped the agent-judgment Ed25519 weakening that would have masked the real bug. **Root cause** (R251 byte-level forensic via `node/src/bin/dump_block.rs` walking the reference Haskell ChainDB chunk 353): Yggdrasil's `crates/ledger/src/cbor.rs::extract_block_tx_byte_spans` used strict `dec.array()` for the outer block, bodies array, and witness-set array. Real preview Babbage blocks (e.g. block at chunk-353 offset `~114079`) encode the bodies-array itself with CBOR indefinite-length (`0x9f ... 0xff`, RFC 8949 §3.2.1). When extraction failed, the apply-path macro `node/src/sync.rs::alonzo_family_block_to_block_with_spans` fell back to `tx_body.to_cbor_bytes()` re-serialization (always definite-length), producing a `tx_body_hash` that differed from the on-wire (indefinite) hash the signer signed. **Fix** (R251): switched all three `array()` calls to `array_begin()` plus a new `collect_indefinite_or_definite_spans` helper that walks both definite and indefinite-length encodings, preserving byte spans exactly. 2 regression tests pin the fix: `extract_block_tx_byte_spans_handles_indefinite_bodies_array` and `extract_block_tx_byte_spans_handles_indefinite_witnesses_array`. **Verification**: 25-min preview soak resumed from saved checkpoint at slot 1,488,359 and advanced cleanly to slot **1,557,718** — past the previous Gap BQ failure point with 32K-slot margin, zero witness verification errors, 17 epoch boundaries crossed. Forensic artifacts preserved at `docs/operational-runs/2026-05-05-round-249-preview-vkey-witness-fail-{slot-1525024.log,tx-44ccae43-bytes.txt}` (capture log + bytes) and `…-classify-signature.py` (canonicality classifier).
- **Operator-facing perf gap (R249/R250 sidefinding)**: in the side-by-side preview soaks, Haskell `cardano-node 10.7.1` syncs at **5,296 slot/s** vs. Yggdrasil at **1,653 slot/s** — Haskell is currently **3.2× faster** on a fresh sync from genesis. The dominant contributor is peer-snapshot configuration: `node/configuration/preview/peer-snapshot.json` ships a 321-byte placeholder with one fake pool, while the upstream Haskell preview share ships a 28 KB snapshot with **131 unique relay addresses across ~50 ledger pools**. Combined with a runtime-side bug where Yggdrasil treats peer-snapshot peers as if gated behind `useLedgerAfterSlot=102_729_600` (Haskell uses snapshot peers immediately at startup as `bigLedgerPeers`, gate only applies to live-ledger-derived peers), Yggdrasil is effectively single-peer until slot `102M+`, while Haskell has multi-peer fetch from genesis. Closing this gap is config + a small runtime fix: replace the placeholder `peer-snapshot.json` with the upstream content, and split the snapshot-vs-live-ledger gating in `node/src/runtime.rs::ledger_peer_snapshot_from_ledger_state`. Target: **2× faster than Haskell** = 6.4× current Yggdrasil throughput.
- **R250 partial close (2026-05-05)**: peer-snapshot adoption + split-gate landed. Replaced placeholder `peer-snapshot.json` for preview/preprod/mainnet with the upstream Haskell-share content (preview 28 KB / 175 pools, preprod 15 KB, mainnet 152 KB / many more pools). Bumped `useLedgerAfterSlot` to upstream-aligned values (preview 107222465, preprod 118022427, mainnet 182044807) and `MinNodeVersion` 10.6.2 → 10.7.0 in all three `config.json`. Split snapshot-vs-live-ledger gating: new `crates/network/src/ledger_peers_provider.rs::always_eligible_snapshot_peers` plus `node/src/config.rs::NodeConfigFile::always_eligible_snapshot_fallbacks` wrapper, called alongside the existing gated `eligible_ledger_peer_candidates` from both `node/src/main.rs::evaluate_ledger_derived_startup_fallbacks` and `node/src/runtime.rs::ledger_peer_snapshot_from_ledger_state`. Snapshot peers now eligible immediately at startup (verified live: trace `evaluated ledger-derived startup fallbacks` shows `snapshotEligibleCount=174 liveLedgerEligibleCount=0 decision=AwaitingLatestSlot { after_slot: 107222465 }`). 2 regression tests pin the new behavior: `snapshot_peers_eligible_before_use_ledger_after_slot` and `live_ledger_peers_remain_gated_when_snapshot_eligible`. **Measured perf delta**: Yggdrasil 1,653 → 2,321 slot/s (40% throughput improvement) across a 5-min preview soak from genesis. **Remaining work**: governor outbound-connect path doesn't yet promote snapshot-eligible peers to BlockFetch workers (`yggdrasil_blockfetch_workers_registered` stays at 0; `active_peers=1`). R254 perf round must wire snapshot peers into the warm/hot promotion loop and add batched Ed25519 verify + pipelined CBOR decode + allocator tuning to clear the 2× Haskell goal.
- Active validation focus: investigate Gap BO (R250 candidate) and Gap BP (R251 candidate). For BO — re-replay the preserved log, narrow the failing block, compare the overlay classification + active-delegate selection + VRF input/seed/key against upstream `Cardano.Protocol.TPraos.Rules.Overlay.classifyOverlaySlot` and `pbftVrfChecks` for the exact slot. For BP — diff `crates/plutus/src/cost_model.rs::step_cost(kind)` against the V2 cost-model B JSON byte-for-byte and audit `crates/plutus/src/machine.rs::spend_step` increments at `Var`/`LamAbs`/`Apply`/`Delay`/`Force`/`Constant`/`Builtin` sites for off-by-one against upstream `Cek/Internal.hs::stepAndMaybeSpend`. Once both close, run clean preview replay through the R248 overlay fix, then complete systematic mainnet endurance rehearsal plus runbook §6.5 sign-off using `node/scripts/parallel_blockfetch_soak.sh` before changing the default `max_concurrent_block_fetch_peers`.
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
| `cardano-ledger` | `ca9b8c285e4493f2d25354914f8aae5483595507` | `node/src/upstream_pins.rs` (R249 audit-baseline refresh; `StAnnTx` Haskell-internal threading + Dijkstra `MemoBytes` `BlockBody` prep + `blockBodySize` rename) |
| `ouroboros-consensus` | `8c2475c253ab53fc2f0998a57a161b6778b54e43` | `node/src/upstream_pins.rs` (R249 audit-baseline refresh; Peras voting-committee implementations + `LedgerTables` type indexing refactor) |
| `ouroboros-network` | `8fe0f8ebc2623079edc7d708f19a0154b963f371` | `node/src/upstream_pins.rs` (R249 audit-baseline refresh; internal `submitTxToMempool` API for sister `dmq-node` + x86_64-darwin Nix removal — wire codec unchanged) |
| `plutus` | `c8f962ae75d0b4871401ecc2e8c4ed259cafadac` | `node/src/upstream_pins.rs` (R249 audit-baseline refresh; post-Conway PV D/E `CInteger`/`CByteString`/`TextCostedByByteLength` types + cost models D/E + Flat decoder additive predicate hooks) |
| `cardano-node` | `97036a66bcf8c89f687ae57a048eecc0389977ef` | `node/src/upstream_pins.rs` (R249 audit-baseline refresh; cardano-testnet CLI `ModeOptions` restructure + experimental-hardfork PV12 bump + cardano-api/cli 11.0) |

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

### Drift snapshot — 2026-05-05 (post-R249 cumulative pin refresh)

`node/scripts/check_upstream_drift.sh` against live `git ls-remote HEAD`:

| Repository | Pinned (audit baseline) | Live HEAD (2026-05-05) | Status |
|---|---|---|---|
| `cardano-base` | `7a8a991945d4…` | (same) | **in-sync** (R239 fixture refresh; vendored vector tree unchanged) |
| `cardano-ledger` | `ca9b8c285e44…` | (same) | **in-sync** (R249 advance) |
| `ouroboros-consensus` | `8c2475c253ab…` | (same) | **in-sync** (R249 advance) |
| `ouroboros-network` | `8fe0f8ebc262…` | (same) | **in-sync** (R249 advance) |
| `plutus` | `c8f962ae75d0…` | (same) | **in-sync** (R249 advance) |
| `cardano-node` | `97036a66bcf8…` | (same) | **in-sync** (R249 advance) |

R249 (2026-05-05) refreshed the 5 pins that had drifted again since R245/R216/R201, after a per-repo audit of every commit in each compare range via the GitHub `compare` API. Inspected hot-path files inline: `plutus-core/{Default/Builtins.hs, Default/Universe.hs, Default/Universe/Cardano.hs, Evaluation/Machine/ExMemoryUsage.hs, untyped-plutus-core/.../Flat.hs}` for plutus; `Inbound/V2/Registry.hs` for ouroboros-network; the active-era CDDL trees for cardano-ledger. The substantive upstream changes are forward-looking — Peras voting-committee implementations (post-Conway), Dijkstra `MemoBytes` `BlockBody` (PV12), post-Conway plutus universe additions and cost models D/E (PV12+/PV13+), `StAnnTx` Haskell-internal threading refactor, and an internal Haskell `submitTxToMempool` return-type change consumed by sister `dmq-node`. The Conway-or-earlier active-protocol surface (rules, CBOR codecs, transition systems, mini-protocol wire codecs, CEK semantics, cost-model parameters) is unchanged. The companion test-isolation fix in `node/src/local_server.rs` lifts `ENV_LOCK` to the test-module scope so the floor-promotion test cannot leak `YGG_LSQ_ERA_FLOOR` into the PV→era_index table-pinning test running concurrently.

The Yggdrasil code surface remains tested against the audit-baseline behavior; the documentary pins record which upstream commit the audit cadence (`cargo check-all`, `cargo test-all`, `cargo lint`, drift-guard tests) was last run against.

## Update Rules

- Update this file whenever a parity milestone or blocker status changes.
- Keep claims evidence-based: include command output or test references for any status changes.
- Keep this file synchronized with `archive/PARITY_PLAN.md`, `docs/PARITY_SUMMARY.md`, `docs/ARCHITECTURE.md`, and `README.md`.
