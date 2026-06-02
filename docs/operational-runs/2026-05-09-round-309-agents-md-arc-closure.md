---
title: 'R309: AGENTS.md Current Phase — R273-R308 arc closure'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-309-agents-md-arc-closure/
---

# Round 309 — AGENTS.md Current Phase: strict 1:1 file-mirror & tech-debt arc closure

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R308`](2026-05-09-round-308-parity-proof-and-scripts-agents.md)  
**Cumulative arc:** R273-rename + R274–R308 strict 1:1 file-mirror &
tech-debt purge arc closure now reflected in the canonical `AGENTS.md`
Current Phase paragraph.

## Summary

`AGENTS.md`'s "Current Phase" paragraph (the canonical living-status
entry pointed at by [`crates/AGENTS.md`](../../crates/AGENTS.md),
[`node/AGENTS.md`](../../node/AGENTS.md), and others) was last extended
at R249 (2026-05-05). It correctly captures R246 / R247 / R249 closure
state but does not mention the R273-R308 arc — 35 rounds of strict 1:1
file-mirror policy work, tech-debt purge, and `crates/cardano-cli/`
bootstrap. R309 appends a single contiguous arc-closure sentence to
the Current Phase paragraph that names the policy decision (strict 1:1
file-mirror), the CI gate (`check-strict-mirror.py` warn-only at R275
→ fail-build at R288), the audit table (445 graded files; 230 `(a)` +
215 `(c)`; zero `(b)` / `(d)`), the new workspace member
(`crates/cardano-cli/` from R289–R295 plus the R296+R297 migration
kickoff), the docs trim (23 → 11 top-level markdown files), the two
new R303 validators, the R308 backfill, and the five-gate baseline.

The "long round-by-round notes below" remain unchanged — line 159's
guidance is "Older 'open follow-up' wording is intentionally preserved
in those dated entries; the current closure state is the paragraph
above plus PARITY_SUMMARY / PARITY_PROOF / UPSTREAM_PARITY". The
post-R164 journal entries (R165–R308) are not back-filled into the
journal because the Current Phase paragraph is now the right place
for arc-level summary, not 144 individual round entries that would
balloon the file.

## Diff inventory

| Path | Change |
|---|---|
| `AGENTS.md` (root, line 158) | Appended an R273-R308 arc-closure sentence to the Current Phase paragraph. Names the policy decision, the CI gate (R275 warn-only → R288 fail-build), the 445-row audit table, the new `crates/cardano-cli/` workspace member with R296+R297 migration kickoff, the docs trim, the two R303 validators, the R308 backfill, and confirms the 4,855-test baseline. |
| `docs/operational-runs/2026-05-09-round-309-agents-md-arc-closure.md` | This round-doc. |

## Verification

```text
$ cargo fmt --all -- --check
(silent — clean)

$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ python3 dev/test/check-parity-matrix.py
parity matrix clean: 8 entries validated against .reference-haskell-cardano-node (reference tag 11.0.1)

$ python3 dev/test/check-fixture-manifest.py
fixture manifest clean: SHA 7a8a991945d401d89e27f53b3d3bb464a354ad4c consistent across pin source, fixture tree, and docs; 2 corpora validated.
```

R309 ships zero Rust changes. `cargo check-all`, `cargo lint`, and
`cargo test-all` were not re-run for this round since the diff is
documentation-only and the post-R308 baseline (4,855 tests passing,
all five gates clean) is already verified.

## Closure criterion

- `AGENTS.md` Current Phase paragraph names the R273-R308 arc closure.
- All four CI parity validators (strict-mirror, parity-matrix,
  fixture-manifest, plus the local-only reference-artifacts) clean.
- The arc-closure narrative agrees with the parallel R308 narrative
  in `docs/PARITY_PROOF.md`.

All three are met.

## Out of scope (R310+ candidates)

- **`docs/UPSTREAM_PARITY.md`** — the third leg of the canonical
  closure-state triad. Was last refreshed at the time of R249's pin
  audit; its arc-closure language for R273-R308 has not been
  cross-walked.
- **Concrete cardano-cli subcommand ports beyond the current 3-command
  surface.** R296 + R297 migrated `Version` + `ShowUpstreamConfig`;
  `QueryTip` migration is gated on extracting an `LsqClient` trait
  abstraction.
- **R266 step 3 — Gap BP per-builtin trace diff** against upstream
  `db-analyser`. Operator-time gated.
- **R267 — mainnet 24h endurance.** Operator-time gated.
