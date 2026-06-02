---
title: 'R321: closure-status doc triad refresh for R313–R320 docstring-classification cleanup'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-321-closure-triad-refresh-r313-r320/
---

# Round 321 — closure-status doc triad refresh for R313–R320

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R320`](2026-05-09-round-320-plutus-strict-mirror-promotions.md)

## Summary

R321 refreshes the four canonical closure-status documents to
reflect the R313–R320 docstring-classification cleanup arc:

- `docs/PARITY_SUMMARY.md` — status banner round count `306+ → 320+`,
  test count `4,855 → 4,856`, audit table `230 (a) + 215 (c) = 445
  → 262 (a) + 186 (c) = 448`, dedicated R313–R320 paragraph.
- `docs/PARITY_PROOF.md` — header round-count + test-count refresh,
  R313–R320 arc note added to the top-of-document blockquote.
- `AGENTS.md` — Current Phase paragraph extended with R310/R311
  + R313–R320 sub-arc closure narrative + final audit-table
  numbers.
- `docs/UPSTREAM_PARITY.md` — five-gate snapshot test count,
  arc-closure blockquote consolidated to cover R273–R320 with
  the post-R320 audit numbers (zero strict-partial).

The R-arc statement converges across all four documents:

> **R273-rename + R274–R311 strict 1:1 file-mirror arc + R313–R320
> docstring-classification cleanup** (closed 2026-05-09). Final audit
> state: 262 `(a) DIRECT_MIRROR` + 186 `(c) strict-none` = 448
> graded files; **zero `(c) strict-partial`**; zero `(c-needed)` /
> zero `(NEEDS-REVIEW)`. Every production `.rs` declares one of
> exactly two canonical docstring forms with zero ambiguity.

## Diff inventory

| Path | Change |
|---|---|
| `docs/PARITY_SUMMARY.md` | Status-banner round count and test count refreshed; R273-rename + R274–R311 arc paragraph extended with R310/R311 detail; new R313–R320 paragraph added with bucket-count delta + per-round summaries; final audit numbers cited (448 graded files; zero strict-partial). |
| `docs/PARITY_PROOF.md` | Document-round line: refresh date `R308 → R321`; cumulative arc `R307+ → R320+`; test count `4,855 → 4,856`; top-of-document blockquote arc title and bucket numbers updated. |
| `AGENTS.md` | Current Phase paragraph extended past the existing R273-R308 arc summary with R310/R311 + R313–R320 detail + final audit numbers. |
| `docs/UPSTREAM_PARITY.md` | Five-gate test count `4,855 → 4,856`; arc-closure blockquote rewritten to cover R273–R320 with consolidated narrative + post-R320 audit numbers. |
| `docs/operational-runs/2026-05-09-round-321-closure-triad-refresh-r313-r320.md` | This round-doc. |

R321 ships zero Rust changes. The post-R320 baseline (4,856 tests
passing, all five gates clean) is preserved by construction.

## Verification

```text
$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ python3 dev/test/check-parity-matrix.py
parity matrix clean: 8 entries validated against
    .reference-haskell-cardano-node (reference tag 11.0.1)

$ python3 dev/test/check-fixture-manifest.py
fixture manifest clean: SHA 7a8a991945d401d89e27f53b3d3bb464a354ad4c
    consistent across pin source, fixture tree, and docs;
    2 corpora validated.
```

## Closure criterion

- All four canonical closure-status documents reflect the R313–R320
  docstring-classification cleanup with consistent narrative and
  numbers.
- Final audit numbers (262 (a) + 186 (c) = 448 graded files;
  zero strict-partial) cited identically across all four documents.
- All four CI parity validators clean.

All three are met.

## Cumulative R313–R321 arc closure

This is the complete close-out for the docstring-classification
sub-arc that the operator triggered with "i expect 1:1 upstream
file mirrors". The R313 census measured the state, R314–R320 did
the cleanup, R321 captures the final numbers in the canonical
status documents.

After R321, the R-arc cadence has fully delivered the operator's
parity expectation at the file-naming + docstring-declaration layer.
The remaining work surfaces (Gap BO TPraos VRF, Gap BP Plutus V2
budget overrun, perf-to-2×-Haskell, R267 mainnet endurance,
concrete cardano-cli subcommand ports) are on different parity
axes and tracked separately under their own R-arc.
