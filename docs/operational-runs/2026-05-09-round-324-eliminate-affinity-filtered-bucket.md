---
title: 'R324: eliminate (a) auto (affinity-filtered) bucket — audit table now binary'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-324-eliminate-affinity-filtered-bucket/
---

# Round 324 — eliminate `(a) DIRECT_MIRROR (auto (affinity-filtered))` bucket

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R323`](2026-05-09-round-323-eliminate-auto-bucket.md)

## Summary

R324 closes the last auto-graded sub-bucket (`(a) DIRECT_MIRROR
(auto (affinity-filtered))`) by hand-auditing each of 18 files and
deciding per-content: promote to canonical strict-mirror declaration
(10 files) or reclassify to synthesis (8 files).

After R324, the audit table has **exactly two sub-buckets**:
- 246 `(a) DIRECT_MIRROR (auto: docstring declares strict mirror)`
- 202 `(c) NO_MIRROR_NEEDS_DOCSTRING (auto: docstring present (strict-none))`

Every production `.rs` file declares one of two canonical docstring
forms with explicit upstream attribution. Zero ambiguity, zero
auto-graded-by-basename, zero unspecified-form leftovers.

## Files promoted to canonical strict-mirror (10 total)

| Rust path | Upstream `.hs` |
|---|---|
| `crates/consensus/src/header.rs` | `Ouroboros/Consensus/Protocol/Praos/Header.hs` |
| `crates/consensus/src/praos/vrf.rs` | `Ouroboros/Consensus/Protocol/Praos/VRF.hs` |
| `crates/crypto/src/ed25519.rs` | `Cardano/Crypto/DSIGN/Ed25519.hs` |
| `crates/crypto/src/sum_kes.rs` | `Cardano/Crypto/KES/Sum.hs` |
| `crates/crypto/src/vrf.rs` | `Cardano/Crypto/VRF.hs` |
| `crates/ledger/src/eras/alonzo.rs` | `Cardano/Ledger/Alonzo.hs` |
| `crates/ledger/src/eras/shelley.rs` | `Cardano/Ledger/Shelley.hs` |
| `crates/network/src/protocols/local_state_query.rs` | `Ouroboros/Network/Protocol/LocalStateQuery/Type.hs` |
| `crates/storage/src/chain_db.rs` | `Ouroboros/Consensus/Storage/ChainDB.hs` |
| `crates/storage/src/ledger_db.rs` | `Ouroboros/Consensus/Storage/LedgerDB.hs` |

## Files reclassified to synthesis (8 total)

These files were auto-graded as `(a) DIRECT_MIRROR` based on
basename match + crate-affinity filter, but content audit showed
the basename-match candidate was misleading or that the Rust file
genuinely combines multiple upstream concerns:

| Rust path | Auto-graded match (REJECTED) | Actual rationale |
|---|---|---|
| `crates/crypto/src/kes.rs` | `Crypto/KES.hs` (re-export umbrella) | Yggdrasil aggregates Single + CompactSingle + Simple variants in one file; upstream `KES.hs` is just a re-exports umbrella. Real implementation lives in upstream `KES/{Single,CompactSingle,Simple}.hs`. |
| `crates/ledger/src/cbor.rs` | `Cardano/Chain/Common/CBOR.hs` (Byron-only) | Workspace-wide CBOR helper used by all eras; the Byron-only basename hit was wrong. |
| `node/src/commands/configuration.rs` | `Tracer/Configuration.hs` | Yggdrasil `validate-config` subcommand handler; upstream `cardano-tracer` config is unrelated. |
| `node/src/commands/validate_config.rs` | `Tracing/Config.hs` | Yggdrasil binary-side `validate-config` runner; no upstream parallel. |
| `node/src/config.rs` | `Tracing/Config.hs` | Yggdrasil binary-side `NodeConfigFile` struct; upstream's equivalent is split across `Cardano.Node.Configuration.POM` + per-subsystem configs. |
| `node/src/local_server.rs` | `Tracer/Acceptors/Server.hs` | Yggdrasil NtC local-socket dispatcher; upstream's equivalent is split across `Ouroboros.Network.NodeToClient` + per-protocol server drivers. |
| `node/src/metrics_server.rs` | `Tracer/Acceptors/Server.hs` | Yggdrasil in-process Prometheus metrics HTTP server; upstream pushes to `cardano-tracer` (separate process). |
| `node/src/server.rs` | `Tracer/Acceptors/Server.hs` | Yggdrasil binary-side inbound NtN server; upstream's `Cardano.Diffusion.NodeToNode.runDiffusionM` carries this concern. |

## Bucket-count delta

| Bucket | R323 | R324 | Δ |
|---|---:|---:|---:|
| `(a) DIRECT_MIRROR (docstring declares strict mirror)` | 236 | **246** | **+10** |
| `(a) DIRECT_MIRROR (auto)` | 0 | 0 | 0 |
| `(a) DIRECT_MIRROR (auto (affinity-filtered))` | 18 | **0** | **−18** |
| **(a) total** | **254** | **246** | **−8** |
| `(c) docstring present (strict-none)` | 194 | **202** | **+8** |
| `(c) docstring present (strict-partial)` | 0 | 0 | 0 |
| **(c) total** | **194** | **202** | +8 |
| **Grand total** | **448** | **448** | 0 |

## Cumulative R313 → R324 progress

| Verdict | R313 baseline | R324 final | Δ |
|---|---:|---:|---:|
| `(a) declares strict mirror` | 187 | **246** | **+59** |
| `(a) auto` (basename only) | 25 | **0** | **−25** |
| `(a) auto (affinity-filtered)` | 18 | **0** | **−18** |
| **(a) total** | **230** | **246** | **+16** |
| `(c) strict-none` | 174 | **202** | **+28** |
| `(c) strict-partial` (peaked at 17) | 0 | **0** | 0 |
| `(c) unspecified` | 41 | **0** | **−41** |

## Verification

```text
$ python3 scripts/audit-strict-mirror.py
audit complete: 448 rust files; candidate_match=391, no_candidate_match=57
auto-grading bucket counts:
  (a): 246
  (c): 202

$ python3 scripts/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ cargo fmt --all -- --check          # clean
$ cargo check --workspace --all-targets   # clean
$ cargo clippy ... -D warnings         # clean
$ cargo test --workspace --all-features
passed: 4856  failed: 0
```

## Final audit-table state

After R324, the audit TSV's verdict column has **exactly two
distinct values**:

```text
$ awk -F'\t' 'NR>1 {print $7}' docs/strict-mirror-audit.tsv | sort | uniq -c
    246 (a) DIRECT_MIRROR (auto: docstring declares strict mirror)
    202 (c) NO_MIRROR_NEEDS_DOCSTRING (auto: docstring present (strict-none))
```

Every production `.rs` file is either:
- A declared 1:1 mirror of a single canonical upstream `.hs`
  (`**Strict mirror:** <upstream/path.hs>`), OR
- A declared synthesis with explicit upstream-symbol cross-references
  (`**Strict mirror:** none. <rationale>`).

No file relies on basename heuristic, affinity-filter heuristic, or
any other auto-grade pathway. The audit's `STRICT_MIRROR_DECL` regex
matches a literal `**Strict mirror:**` line in every file's
docstring.

## Closure criterion

- `(a) DIRECT_MIRROR (auto)` sub-bucket: empty (R323).
- `(a) DIRECT_MIRROR (auto (affinity-filtered))` sub-bucket: empty (R324).
- All 246 `(a) DIRECT_MIRROR` rows have explicit
  `**Strict mirror:** <path>` declaration.
- All 202 `(c)` rows have explicit `**Strict mirror:** none.`
  declaration with rationale.
- All 5 workspace gates green at 4,856-test baseline.

All five are met.

## Closure of the R313–R324 docstring-classification meta-arc

R313 measured the state. R314–R324 cleaned it up. The cumulative
arc closure:

- 41 files originally `(c) unspecified` → 0 remaining.
- 17 files originally `(c) strict-partial` (peaked) → 0 remaining.
- 25 files originally `(a) auto` (basename only) → 0 remaining.
- 18 files originally `(a) auto (affinity-filtered)` → 0 remaining.

The audit table's classification gate is now byte-perfect: every
`(a)` row has a declarative strict-mirror line; every `(c)` row has
a declarative synthesis line. The audit script's basename-match
heuristic + affinity-filter heuristic are no longer load-bearing
for any file's classification — they remain in place for future
new-file gating but no current file relies on them.

This is the cleanest possible state for the strict-mirror policy
short of doing further code restructure to convert synthesis
files into mirrors (which would mean creating files just to match
upstream module-tree layout — over-fitting Rust idiom to Haskell
convention).
