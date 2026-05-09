---
title: 'R325: closure-status doc triad refresh for R322–R324'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-325-closure-triad-refresh-r322-r324/
---

# Round 325 — closure-status doc triad refresh for R322–R324

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R324`](2026-05-09-round-324-eliminate-affinity-filtered-bucket.md)

## Summary

R325 refreshes the four canonical closure-status documents and
the CHANGELOG to reflect R322 (CHANGELOG backfill), R323
(`(a) auto` bucket elimination), and R324 (`(a) auto (affinity-
filtered)` bucket elimination). All five surfaces previously
cited the post-R321 numbers (`262 (a) + 186 (c) = 448`); the
post-R324 numbers are `246 (a) + 202 (c) = 448`.

## Diff inventory

| Path | Change |
|---|---|
| `docs/PARITY_SUMMARY.md` | Bucket numbers `262 + 186` → `246 + 202`; added narrative about R322 backfill + R323/R324 hand-audits (43 files: 27 promoted, 16 reclassified). |
| `docs/PARITY_PROOF.md` | Top-of-document blockquote bucket numbers refreshed; added "**zero `(a) auto`** + **zero `(a) auto (affinity-filtered)`**" claim. |
| `docs/UPSTREAM_PARITY.md` | Arc-closure blockquote bucket numbers refreshed; added "every (a) row has explicit upstream-path declaration" narrative. |
| `AGENTS.md` Current Phase | Audit-state line extended past R321 with R322 + R323 + R324 follow-up cleanup detail; new "Final audit state (post-R324)" line cites `246 (a) + 202 (c)` and "audit table now has exactly two canonical verdicts". |
| `CHANGELOG.md` | `[Unreleased]` header summary refreshed; 3 new bullets for R322/R323/R324. |
| `docs/operational-runs/2026-05-09-round-325-closure-triad-refresh-r322-r324.md` | This round-doc. |

R325 ships zero Rust changes. The post-R324 baseline (4,856 tests
passing, all five gates clean, audit table binary at 246 (a) + 202
(c) = 448) is preserved by construction.

## Verification

```text
$ python3 scripts/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ python3 scripts/check-parity-matrix.py
parity matrix clean: 8 entries validated

$ python3 scripts/check-fixture-manifest.py
fixture manifest clean: SHA 7a8a991945… consistent
```

## Closure criterion

- All four canonical closure-status documents
  (`PARITY_SUMMARY.md`, `PARITY_PROOF.md`, `UPSTREAM_PARITY.md`,
  `AGENTS.md` Current Phase) cite the post-R324 final audit
  numbers (`246 (a) + 202 (c) = 448`).
- `CHANGELOG.md` `[Unreleased]` carries entries for R322, R323,
  R324 and the header summary reflects the post-R324 state.
- All four CI parity validators clean.

All three are met.

## Cumulative arc closure surface coverage

After R325, all six historical-evidence surfaces uniformly reflect
the post-R324 final state:

| Surface | Latest refresh | Numbers cited |
|---|---|---|
| `docs/PARITY_SUMMARY.md` | R325 | 246 + 202 = 448 |
| `docs/PARITY_PROOF.md` | R325 (header) | 246 + 202 = 448 |
| `AGENTS.md` Current Phase | R325 | 246 + 202 = 448 |
| `docs/UPSTREAM_PARITY.md` | R325 | 246 + 202 = 448 |
| `docs/strict-mirror-audit.tsv` | R324 (TSV regen) | 246 / 202 split per row |
| `CHANGELOG.md` | R325 | 246 + 202 = 448 |

The R313–R325 cumulative meta-arc (audit-table classification cleanup)
is fully closed and durably captured. The audit table is byte-perfect
binary; every production `.rs` declares one of two canonical docstring
forms with explicit upstream attribution.

## Out of scope (R326+ candidates)

The strict-mirror file-naming + audit-classification surface is now
fully clean. Remaining work surfaces belong to different parity axes:

- **Concrete cardano-cli subcommand ports beyond `Version` /
  `ShowUpstreamConfig`** — multi-week scope; gated on extracting
  an `LsqClient` trait abstraction for `QueryTip`.
- **Gap BO** — preprod TPraos VRF parity gap at slot ~429,460.
  Operator-time forensic; needs per-block VRF input/seed comparison
  against upstream.
- **Gap BP** — preview Plutus V2 cost-budget overrun at slot
  ~1,462,057. R266 step 3 forensic; needs per-builtin trace diff
  against upstream `db-analyser`.
- **Perf-to-2×-Haskell** — governor outbound-connect path needs
  to promote snapshot-eligible peers to warm/hot for multi-peer
  BlockFetch dispatch. R254-candidate.
- **R267 mainnet endurance** — operator-time gated.

These have been listed in every recent round-doc's "out of scope"
section. The cadence has now exhausted the docstring-classification
+ file-naming improvements available without doing actual code-
restructure work that would over-fit Rust idiom to Haskell module-
tree convention.
