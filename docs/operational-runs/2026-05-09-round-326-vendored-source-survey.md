---
title: 'R326: vendored sister-tool source survey + scope corrections'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-326-vendored-source-survey/
---

# Round 326 — vendored sister-tool source survey + scope corrections

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R325`](2026-05-09-round-325-closure-triad-refresh-r322-r324.md)  
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), prep block.

## Summary

R326 is the first round of the multi-quarter sister-tools port arc.
The plan assigned R326 to "vendor missing upstream repos" — extend
`dev/reference/setup-reference.sh` with `bech32`, `kes-agent`, `dmq-node`
clones. Implementation surfaced two corrections:

1. **`.hs` file counts are 2-3× larger than the Plan agent's
   estimates.** Refreshed `docs/upstream-haskell-files.txt` and
   counted per-tool actuals. Updated round-count estimates below.

2. **The 3 missing tools (bech32, kes-agent, kes-agent-control,
   dmq-node) come from cardano-haskell-packages (CHaP), NOT from
   git submodules.** Upstream `cardano-node`'s `cabal.project` line
   `extra-packages: alex, dmq-node >= 0.4.2.0` consumes them as
   Hackage-style packages. The canonical IntersectMBO repo URLs
   for source vendoring aren't documented in the cardano-node
   project's cabal.project. Confirming the canonical URLs requires
   either operator authorization for external GitHub probes or
   operator-provided URLs.

R326 therefore proceeds as a **verification-only / scope-correction
round**: no `setup-reference.sh` edits, no new clones. The
`upstream-haskell-files.txt` index is refreshed (byte-identical to
HEAD — no drift). Corrected file counts are recorded below for the
plan's per-tool round-count adjustment.

## Vendored source verification (9 of 12 tools)

| Tool | Vendored source path | `.hs` count |
|---|---|---:|
| cardano-submit-api | `.reference-haskell-cardano-node/cardano-submit-api/` | 14 |
| cardano-testnet | `.reference-haskell-cardano-node/cardano-testnet/` | **82** ⚠️ (estimate was 32) |
| cardano-tracer | `.reference-haskell-cardano-node/cardano-tracer/` | **93** ⚠️ (estimate was 77) |
| tx-generator | `.reference-haskell-cardano-node/bench/tx-generator/` | **46** ⚠️ (estimate was 22) |
| db-analyser (lib) | `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBAnalyser/` | 11 |
| db-analyser (app) | `.../app/db-analyser.hs` + `.../app/DBAnalyser/Parsers.hs` | 2 |
| db-synthesizer (lib) | `.../unstable-cardano-tools/Cardano/Tools/DBSynthesizer/` | 4 |
| db-synthesizer (app) | `.../app/db-synthesizer.hs` + `.../app/DBSynthesizer/Parsers.hs` | 2 |
| db-truncater (lib) | `.../unstable-cardano-tools/Cardano/Tools/DBTruncater/` | 2 |
| db-truncater (app) | `.../app/db-truncater.hs` + `.../app/DBTruncater/Parsers.hs` | 2 |
| snapshot-converter | `.../app/snapshot-converter.hs` | 1 |

**Total vendored `.hs` for the 9 sister tools:** 259 files (vs Plan
agent's estimate of ~159).

## Unvendored tools (3 of 12)

| Tool | Binary version (from `--version`) | Notes |
|---|---|---|
| bech32 | `1.1.10` | Canonical IntersectMBO repo URL unknown — likely `github.com/IntersectMBO/bech32` but unverified. |
| kes-agent | `1.2.0.0-dev-20260505005424` | Canonical URL unknown — likely `github.com/IntersectMBO/kes-agent`. Includes both `kes-agent` and `kes-agent-control` binaries. |
| dmq-node | `0.4.2.0` | Consumed via CHaP per `extra-packages: alex, dmq-node >= 0.4.2.0` in upstream cabal.project. Repository location unknown — likely `github.com/IntersectMBO/dmq` or part of a larger ouroboros-network sister tree. |

**Operator action required**: Provide canonical IntersectMBO URLs for
the 3 unvendored tools, OR authorize external GitHub probing so the
implementation agent can verify URLs via `curl`.

These 3 sources are needed before Tier 1 entry (R331+, which begins
with `bech32` and includes `kes-agent` + `kes-agent-control` at
R344-R359). Tier 4 entry (R450+ for `dmq-node`) needs the third URL.

## Plan adjustments

The corrected `.hs` counts modify the per-tool round budget:

| Tool | Plan agent count | Actual count | Round-count delta |
|---|---:|---:|---|
| cardano-testnet | 32 | 82 | scope grows ~2.5× — Phase C.2 may need 24 rounds vs planned 18 |
| cardano-tracer | 77 | 93 | scope grows ~1.2× — Phase A.5 may need 30 rounds vs planned 26 |
| tx-generator | 22 | 46 | scope grows ~2× — Phase C.3 may need 24 rounds vs planned 16 |
| Others | (matches) | (matches) | no change |

**Net plan impact**: ~+18 rounds in Phase A and Phase C if no further
carve-outs apply. Final per-tool round budget reflows once the
skeleton rounds (R335 cardano-submit-api, R360 cardano-tracer, R416
cardano-testnet, R434 tx-generator) survey their actual file
distribution.

## Diff inventory

| Path | Change |
|---|---|
| `docs/upstream-haskell-files.txt` | Refreshed (byte-identical to HEAD — no drift since R325). |
| `docs/operational-runs/2026-05-09-round-326-vendored-source-survey.md` | This round-doc. |

R326 ships zero code changes. The post-R325 baseline (4,856 tests
passing, all 5 cargo gates clean, 4 CI parity validators clean,
audit-table binary at 246 (a) + 202 (c) = 448) is preserved.

## Verification

```text
$ cargo fmt --all -- --check
(silent — clean)

$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ cargo check --workspace --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.63s

$ python3 dev/test/check-parity-matrix.py
parity matrix clean: 8 entries validated against
    .reference-haskell-cardano-node (reference tag 11.0.1)

$ python3 dev/test/check-fixture-manifest.py
fixture manifest clean: SHA 7a8a991945d401d89e27f53b3d3bb464a354ad4c
    consistent across pin source, fixture tree, and docs;
    2 corpora validated.

$ wc -l docs/upstream-haskell-files.txt
4676 docs/upstream-haskell-files.txt
```

## Closure criterion

- 9 of 12 sister tools' source paths verified + `.hs` counts
  recorded.
- 3 unvendored tools (bech32, kes-agent[-control], dmq-node)
  documented as URL-pending; operator action requested.
- `docs/upstream-haskell-files.txt` refreshed (no drift).
- All 5 cargo gates + 3 CI parity validators clean.

All four are met.

## Out of scope (R327+ next steps)

- **R327 — Workspace layout + Cargo skeleton stubs**: proceeds for
  all 12 crates. Skeleton stubs don't need upstream source — empty
  `lib.rs` + `main.rs` + AGENTS.md per crate. The 3 URL-pending
  tools get the same skeleton; their per-file mirror tree lands in
  the per-tool skeleton round (R331 bech32, R344 kes-agent,
  R355 kes-agent-control, R450 dmq-node).
- **R326b (deferred)** — Once operator confirms the 3 canonical
  IntersectMBO repo URLs (or authorizes external GitHub probing),
  re-enter R326 to extend `setup-reference.sh` and run
  `bash dev/reference/setup-reference.sh --force` for the new repos.
  This is a hard prerequisite for Tier 1 entry (R331).

The plan continues to R327 (Cargo skeleton stubs) without delay,
since skeleton work doesn't depend on the unvendored sources.
