---
title: 'R312: docs/UPSTREAM_PARITY.md arc-closure cross-walk'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-312-upstream-parity-arc-closure/
---

# Round 312 — `docs/UPSTREAM_PARITY.md` arc-closure cross-walk

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R311`](2026-05-09-round-311-strict-mirror-index-drift-check.md)

## Summary

Closes the canonical-status doc triad refresh. R307 refreshed
`docs/PARITY_SUMMARY.md`'s round/test-count banner; R308 refreshed
`docs/PARITY_PROOF.md`'s header; R309 refreshed `AGENTS.md`'s Current
Phase. The third leg — `docs/UPSTREAM_PARITY.md` — was last touched
at R249's pin audit (2026-05-05) and made no mention of the R273-R311
strict 1:1 file-mirror & tech-debt arc.

R312 adds a top-of-document arc-closure blockquote (parallel in
shape to PARITY_PROOF's R308 blockquote) and a "Five-gate snapshot
(post-R311, 2026-05-09)" subsection at the top of the Verification
Baseline. The arc-closure note explicitly states what the R273-R311
work did NOT change (subsystem parity status, wire/codec/rule
surfaces) so a reader doesn't conflate the file-naming arc with
ledger/consensus/network parity work — those gaps (BO, BP, perf
sidefindings, R250 partial close) are unchanged and remain in the
Open Gaps section unedited.

The historical R244–R249 closure-evidence bullets are preserved
verbatim under a new "Historical R244–R249 closure evidence"
subhead; they remain the canonical evidence for §1–§9 closures
captured during that earlier arc.

## Diff inventory

| Path | Change |
|---|---|
| `docs/UPSTREAM_PARITY.md` | "Last updated" line refreshed (`2026-05-05; header + verification-baseline refreshed 2026-05-09 (R312)`); added top-of-document arc-closure blockquote covering R273-R311 (policy tag `11.0.1`, strict-mirror gate R275 → R288, 445-graded audit table, `crates/cardano-cli/` workspace member, R296+R297 migration kickoff, two new R303 validators, R308+R309 backfill, R310 gitignore fix, R311 drift check); split Verification Baseline into "Five-gate snapshot (post-R311)" + "Historical R244–R249 closure evidence" subheads. |
| `docs/operational-runs/2026-05-09-round-312-upstream-parity-arc-closure.md` | This round-doc. |

R312 ships zero Rust changes and zero edits to historical evidence
sections (Open Gaps, Pinned commits, Drift snapshots, Update Rules).
The 2026-05-05 R249 drift snapshot remains the canonical entry for
the documentary pin matrix.

## Verification

```text
$ python3 scripts/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ python3 scripts/check-parity-matrix.py
parity matrix clean: 8 entries validated against
    .reference-haskell-cardano-node (reference tag 11.0.1)

$ python3 scripts/check-fixture-manifest.py
fixture manifest clean: SHA 7a8a991945d401d89e27f53b3d3bb464a354ad4c
    consistent across pin source, fixture tree, and docs;
    2 corpora validated.
```

The cargo gates were not re-run for this round since the diff is
documentation-only; the post-R311 baseline (4,855 tests passing,
all five gates clean) is preserved by construction.

## Closure criterion

- `docs/UPSTREAM_PARITY.md` header reflects the post-R311 state.
- The R273-R311 arc closure narrative agrees with the parallel
  refresh narratives in `docs/PARITY_SUMMARY.md` (R307),
  `docs/PARITY_PROOF.md` (R308), and `AGENTS.md` Current Phase
  (R309).
- The arc-closure note explicitly states what was NOT changed so
  a reader doesn't conflate file-naming work with subsystem parity.
- All four CI parity validators (strict-mirror, parity-matrix,
  fixture-manifest, plus the local-only reference-artifacts) clean.

All four are met.

## Out of scope (R313+ candidates)

Closure-status doc triad refresh is now complete. The remaining
post-R311 work surfaces are operational, not documentation:

- **`docs/MANUAL_TEST_RUNBOOK.md`** — operator-facing runbook;
  unchanged through the R273-R311 arc since none of it changed
  the operator runbook's §6.5 parallel-fetch rehearsal or §2-9
  mainnet endurance procedures. May benefit from a one-line
  policy-tag note (currently 11.0.1) but no narrative changes
  required.
- **Concrete cardano-cli subcommand ports beyond the current
  3-command surface.** R296+R297 migrated `Version` +
  `ShowUpstreamConfig`; `QueryTip` migration is gated on
  extracting an `LsqClient` trait abstraction.
- **R266 step 3 — Gap BP per-builtin trace diff** against
  upstream `db-analyser`. Operator-time gated.
- **R267 — mainnet 24h endurance.** Operator-time gated.
