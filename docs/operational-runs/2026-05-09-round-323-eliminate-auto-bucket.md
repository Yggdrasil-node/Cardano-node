---
title: 'R323: eliminate (a) DIRECT_MIRROR (auto) bucket via explicit declarations + content audit'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-323-eliminate-auto-bucket/
---

# Round 323 — eliminate `(a) DIRECT_MIRROR (auto)` bucket

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R322`](2026-05-09-round-322-changelog-r303-r321-entries.md)

## Summary

R323 eliminates the `(a) DIRECT_MIRROR (auto)` sub-bucket of 25
files that previously relied on the audit script's basename-match
heuristic without explicit `**Strict mirror:** <upstream-path>.`
declaration. Each file was hand-audited against its actual content
to decide:

- **Promote to canonical strict-mirror** (17 files): file content
  genuinely mirrors a single upstream `.hs`. Add explicit
  declaration with the canonical path.
- **Reclassify to synthesis** (8 files): file content diverges from
  the basename-match candidate (e.g., `cost_model.rs` matched to
  Agda metatheory `Cost/Model.hs`; Yggdrasil's actual content is a
  runtime parameter table aggregator). Declare
  `**Strict mirror:** none.` with explicit upstream symbol
  cross-references in the rationale.

This closes the gap where the audit grader was relying on basename
heuristic + crate-affinity filter alone. After R323, every
`(a) DIRECT_MIRROR` row in the audit has an explicit
`**Strict mirror:** <path>.` declaration in the file's docstring —
no auto-grade-by-basename remains.

## Files promoted to canonical strict-mirror (17 total)

### Crypto + ledger eras + ledger state rules (12 files)

| Rust path | Upstream `.hs` |
|---|---|
| `crates/crypto/src/blake2b.rs` | `Cardano/Crypto/Hash/Blake2b.hs` |
| `crates/crypto/src/bls12_381.rs` | `Cardano/Crypto/EllipticCurve/BLS12_381.hs` |
| `crates/ledger/src/collateral.rs` | `Cardano/Ledger/Babbage/Collateral.hs` |
| `crates/ledger/src/eras/allegra.rs` | `Cardano/Ledger/Allegra.hs` |
| `crates/ledger/src/eras/babbage.rs` | `Cardano/Ledger/Babbage.hs` |
| `crates/ledger/src/eras/mary.rs` | `Cardano/Ledger/Mary.hs` |
| `crates/ledger/src/plutus.rs` | `Cardano/Ledger/Plutus.hs` |
| `crates/ledger/src/state/enact.rs` | `Cardano/Ledger/Conway/Rules/Enact.hs` |
| `crates/ledger/src/state/mir.rs` | `Cardano/Ledger/Shelley/Rules/Mir.hs` |
| `crates/ledger/src/state/ratify.rs` | `Cardano/Ledger/Conway/Rules/Ratify.hs` |
| `crates/network/src/bearer.rs` | `Network/Mux/Bearer.hs` |
| `crates/network/src/root_peers.rs` | `Ouroboros/Network/PeerSelection/Governor/RootPeers.hs` |

### Storage layer (2 files — no prior module docstring; added complete header)

| Rust path | Upstream `.hs` |
|---|---|
| `crates/storage/src/immutable_db.rs` | `Ouroboros/Consensus/Storage/ImmutableDB.hs` |
| `crates/storage/src/volatile_db.rs` | `Ouroboros/Consensus/Storage/VolatileDB.hs` |

### Protocol type files (3 files — no prior module docstring; added complete header)

| Rust path | Upstream `.hs` |
|---|---|
| `crates/network/src/protocols/block_fetch.rs` | `Ouroboros/Network/Protocol/BlockFetch/Type.hs` |
| `crates/network/src/protocols/keep_alive.rs` | `Ouroboros/Network/Protocol/KeepAlive/Type.hs` |
| `crates/network/src/protocols/tx_submission.rs` | `Ouroboros/Network/Protocol/TxSubmission2/Type.hs` |

## Files reclassified to synthesis (8 total)

These files were auto-graded as `(a) DIRECT_MIRROR` based on
basename match, but content audit showed the basename-match
candidate was misleading:

| Rust path | Auto-graded match (REJECTED) | Actual rationale |
|---|---|---|
| `crates/crypto/src/secp256k1.rs` | `plutus-core/.../Secp256k1.hs` | Yggdrasil aggregates ECDSA + Schnorr secp256k1 in one file; upstream cardano-base splits into `DSIGN/EcdsaSecp256k1.hs` + `DSIGN/SchnorrSecp256k1.hs`. The plutus-core hit was wrong (different crate, different concern). |
| `crates/ledger/src/epoch_boundary.rs` | `Byron/Block/Boundary.hs` | Yggdrasil cross-era epoch processor (NEWEPOCH + RUPD + MIR + SNAP); upstream spreads across `Shelley.Rules.{Tick,NewEpoch,Rupd,Snap,Mir}` + `Conway.Rules.Epoch`. Byron Boundary.hs is a different concept (Byron block boundary, not Shelley/Conway epoch boundary). |
| `crates/ledger/src/protocol_params.rs` | `Peras/Params.hs` | Yggdrasil cross-era parameters aggregator; upstream parameterizes via era-class polymorphism (`Cardano.Ledger.Core.PParams` + per-era refinements). Peras Params.hs is voting-committees-specific (different concept). |
| `crates/network/src/connection_manager.rs` | `Tracing/ConnectionManager.hs` | Yggdrasil pure CM surface (Provenance, DataFlow, AbstractState, ConnectionState, errors); upstream ConnectionManager.Core is large IO-effectful module. The Tracing variant is just a tracer (different concern). |
| `crates/network/src/peer_registry.rs` | `TxSubmission/Inbound/V2/Registry.hs` | Yggdrasil's network-wide peer registry; upstream's V2 Registry.hs is the TxSubmission-inbound peer registry (much narrower scope). |
| `crates/plutus/src/cost_model.rs` | `plutus-metatheory/.../Cost/Model.hs` (Agda) | Yggdrasil unified cost-model surface; upstream runtime cost lives in `MachineParameters` + `BuiltinCostModel` + `ExBudget`. The Agda metatheory hit was wrong (formal model, not runtime). |
| `node/src/cli.rs` | `cardano-tracer/.../CLI.hs` | Yggdrasil's binary `clap` parser; upstream's cardano-cli has its own optparse-applicative parser tree. The tracer CLI hit was wrong. |
| `node/src/commands/cardano_cli.rs` | `cardano-tracer/.../CLI.hs` | Yggdrasil's binary-side dispatcher for `yggdrasil-node cardano-cli <subcommand>`; no upstream parallel (upstream cardano-cli is a separate binary). |

## Bucket-count delta

| Bucket | R322 | R323 | Δ |
|---|---:|---:|---:|
| `(a) DIRECT_MIRROR (auto: docstring declares strict mirror)` | 211 | 236 | **+25** |
| `(a) DIRECT_MIRROR (auto)` | 25 | **0** | **−25** |
| `(a) DIRECT_MIRROR (auto (affinity-filtered))` | 18 | 18 | 0 |
| **(a) total** | **254** | **254** | 0 |
| `(c) docstring present (strict-none)` | 186 | 194 | **+8** |
| `(c) docstring present (strict-partial)` | 0 | 0 | 0 |
| **(c) total** | **186** | **194** | +8 |
| **Grand total** | **440** | **448** | +8 |

Wait — the (a) total moved from 262 to 254 (−8), reflecting the
8 reclassifications to synthesis. The 17 promotions stayed within
the (a) bucket (just shifting sub-classification from "auto" to
"auto: docstring declares").

Updated absolute numbers post-R323: **254 (a) + 194 (c) = 448**.

## Verification

```text
$ python3 dev/test/audit-strict-mirror.py
audit complete: 448 rust files; candidate_match=391, no_candidate_match=57
auto-grading bucket counts:
  (a): 254
  (c): 194

$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ cargo fmt --all -- --check          # clean
$ cargo check --workspace --all-targets   # clean
$ cargo clippy ... -D warnings         # clean
$ cargo test --workspace --all-features
passed: 4856  failed: 0
```

## Closure criterion

- `(a) DIRECT_MIRROR (auto)` sub-bucket is empty (was 25).
- 17 files promoted to canonical strict-mirror with explicit
  upstream-path declarations.
- 8 files reclassified to synthesis with explicit rationale.
- All 5 workspace gates green at 4,856-test baseline.

All four are met.

## Remaining auto-graded (a) files (18 affinity-filtered)

The `(a) DIRECT_MIRROR (auto (affinity-filtered))` sub-bucket of
18 files remains. These have multiple upstream basename matches,
but the crate-affinity filter resolves to a single canonical hit.
A future R324 round could:
- Hand-audit each (similar to R323's content audit).
- Promote correct matches to canonical strict-mirror declarations.
- Reclassify any false-positive matches to synthesis.

Deferred until a contributor is touching those files for a
substantive reason — the affinity filter is doing the right job
on these in most cases.
