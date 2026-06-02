---
title: 'R315: reclassify Yggdrasil-side aggregator/glue files to strict-none synthesis'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-315-reclassify-synthesis-files/
---

# Round 315 — reclassify Yggdrasil-side aggregator/glue files to strict-none synthesis

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R314`](2026-05-09-round-314-promote-partial-mirrors.md)  
**Trigger:** R314 left 17 files in `(c) strict-partial` form. Of those,
8 were misclassified — they have no upstream 1:1 parallel and should
declare canonical synthesis form (`**Strict mirror:** none.`) rather
than the misleading `**Strict mirror (partial):**` form.

## Summary

R315 reclassifies 8 Yggdrasil-side aggregator / glue files from
`(c) docstring present (strict-partial)` to
`(c) docstring present (strict-none)`. None of these files map
1:1 to a single upstream `.hs`; the `(partial)` qualifier was being
used to mean "Yggdrasil-side composition that surfaces concepts from
multiple upstream files" — but that's exactly what synthesis is.
The honest declaration is `**Strict mirror:** none.` plus an
explanation of which upstream symbols/files the synthesis surfaces.

The 9 remaining `(c) strict-partial` files are genuine partial
mirrors (combine multiple upstream files at the source-symbol level
or split one upstream file across two adjacent Rust files). Those
are addressed by R316+ (potential refactors to achieve full 1:1).

## Files reclassified (8 total)

| Rust path | Lines | Synthesis story (now declared) |
|---|---:|---|
| `crates/consensus/src/genesis_density.rs` | 325 | Yggdrasil-side density-comparison estimator surfacing `densityComparison` from upstream `Ouroboros.Consensus.Genesis.Governor` |
| `crates/consensus/src/in_future.rs` | 200 | Yggdrasil-side `ClockSkew` + slot-vs-wallclock check surfacing `clockSkew` / `inFutureCheck` from upstream `Ouroboros.Consensus.MiniProtocol.ChainSync.Client.InFutureCheck` |
| `crates/crypto/src/sha3_hash.rs` | 56 | Yggdrasil-side SHA3-256 wrapper for Byron `ADDRHASH` formula; surfaces the SHA3-256 facet of upstream class-parameterized `Cardano.Crypto.Hashing` |
| `crates/ledger/src/rewards.rs` | 2560 | Yggdrasil-side cross-era reward aggregator surfacing the formal Shelley spec §10 reward formula (`reward`, `maxPool`) from upstream `Cardano.Ledger.Shelley.Rewards` |
| `crates/ledger/src/utxo.rs` | 1879 | Yggdrasil-side cross-era UTxO aggregator carrying `MultiEraTxOut` enum variants; surfaces `UTxO` from upstream `Cardano.Ledger.UTxO` |
| `crates/network/src/blockfetch_pool.rs` | 929 | Yggdrasil-side multi-peer BlockFetch scheduler combining per-peer state (upstream `BlockFetch.ClientState`), decision policy (upstream `BlockFetch.Decision`), and a Yggdrasil-specific in-order reorder buffer with no upstream parallel |
| `crates/network/src/listener.rs` | 177 | Yggdrasil-side TCP listener + pre-handshake rate-limit gate; surfaces accept-loop pattern from upstream `Ouroboros.Network.Server2` |
| `node/src/runtime/keep_alive.rs` | 121 | Yggdrasil-side runtime adaptor wrapping the protocol-side `KeepAliveClient` with a 20s scheduler |

## Bucket-count delta

| Bucket | R314 | R315 | Δ |
|---|---:|---:|---:|
| `(a) DIRECT_MIRROR (auto: docstring declares strict mirror)` | 211 | 211 | 0 |
| `(a) DIRECT_MIRROR (auto)` | 25 | 25 | 0 |
| `(a) DIRECT_MIRROR (auto (affinity-filtered))` | 18 | 18 | 0 |
| **(a) total** | **254** | **254** | **0** |
| `(c) docstring present (strict-none)` | 174 | 182 | **+8** |
| `(c) docstring present (strict-partial)` | 17 | 9 | **−8** |
| **(c) total** | **191** | **191** | **0** |
| **Grand total** | **445** | **445** | 0 |

## Files remaining as `(c) strict-partial` (9 total)

These are genuine partial mirrors — refactor candidates for R316+:

| Rust path | Why genuinely partial | Refactor track |
|---|---|---|
| `crates/network/src/mux.rs` (1127) | Split of upstream `Ouroboros.Network.Mux.hs` low-level half | R316 candidate: merge with `multiplexer.rs` |
| `crates/network/src/multiplexer.rs` (276) | Split of upstream `Ouroboros.Network.Mux.hs` high-level half | R316 candidate: merge with `mux.rs` |
| `crates/network/src/handshake.rs` (694) | Combines `Handshake/{Type,Version,Codec}.hs` (3 files) | R317 candidate: split into 3 files matching upstream |
| `crates/network/src/inbound_governor.rs` (1478) | Combines `InboundGovernor.hs` + `InboundGovernor/State.hs` | Defer: state struct + step function tightly coupled in Rust idiom |
| `crates/network/src/governor/churn.rs` (371) | Split of upstream `Governor.hs` churn-cycle concern | Defer: small concern-specific split, splitting further is artificial |
| `crates/network/src/governor/peer_metric.rs` (479) | Combines `PeerMetric.hs` + `LedgerPeers/Utils.hs` peer-pick policy | Defer: small helpers used together |
| `crates/consensus/src/praos/common.rs` (284) | Combines `Cardano.Ledger.BaseTypes::ActiveSlotCoeff` + `Praos/VRF.hs` math helpers | Defer: tiny module, splitting would be artificial |
| `crates/plutus/src/builtins.rs` (1496) | Already part of a larger PlutusCore split (siblings: `cost_model/*.rs`, `types/*.rs`) | Defer: further split would harm cohesion |
| `crates/plutus/src/machine.rs` (1471) | Yggdrasil split of `Cek/Internal.hs` (driver vs types vs cost) — siblings already exist | Defer: already split as much as makes sense |

R316–R317 (mux merge + handshake split) close 4 more files; the
remaining 5 are intentional Yggdrasil idioms that the policy
correctly classifies as partial.

## Diff inventory

| Path | Change |
|---|---|
| 8 production `.rs` files | `**Strict mirror (partial):** ...` blocks rewritten to `**Strict mirror:** none. Yggdrasil-side ...` blocks. New text preserves all upstream symbol references (so a parity researcher still gets the cross-walk) but declares synthesis honestly. |
| `docs/strict-mirror-audit.tsv` | Re-generated; bucket counts shifted per delta table. |
| `docs/operational-runs/2026-05-09-round-315-reclassify-synthesis-files.md` | This round-doc. |

R315 ships zero Rust code changes — only docstring text. The 4,855-test
workspace baseline is preserved by construction.

## Verification

```text
$ python3 dev/test/audit-strict-mirror.py
audit complete: 445 rust files; candidate_match=387, no_candidate_match=58
auto-grading bucket counts:
  (a): 254
  (c): 191

$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ cargo fmt --all -- --check
(silent — clean)

$ cargo check --workspace --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 17.01s

$ cargo clippy --workspace --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.94s

$ cargo test --workspace --all-features
passed: 4855  failed: 0
```

## Closure criterion

- 8 Yggdrasil-side aggregator / glue files now declare canonical
  `**Strict mirror:** none.` synthesis form.
- Each new docstring preserves upstream symbol cross-references for
  parity-research traceability.
- All five workspace gates green at 4,855-test baseline.
- All four CI parity validators clean.
- `(c) docstring present (unspecified)` bucket remains at 0 (R314
  closure intact).

All five are met.

## Cumulative arc progress (R313 → R315)

| Verdict | R313 baseline | R315 final | Δ vs R313 |
|---|---:|---:|---:|
| `(a) DIRECT_MIRROR` (any auto-grade) | 230 | 254 | **+24** |
| `(c) strict-none` | 174 | 182 | **+8** |
| `(c) strict-partial` | 0 | 9 | **+9** |
| `(c) unspecified` | 41 | 0 | **−41** |

41 originally-misclassified files split into:
- 24 promoted to canonical strict-mirror declarations (R314)
- 8 reclassified to canonical synthesis declarations (R315)
- 9 left as canonical partial declarations (genuine partial mirrors)

After R315, every production `.rs` file uses one of three canonical
docstring forms: `**Strict mirror:** <path>.` (254 files),
`**Strict mirror:** none.` (182 files), or
`**Strict mirror (partial):**` (9 files). Zero ambiguity remains
in the audit table.
