---
title: 'R326b: vendor bech32 + kes-agent + dmq-node source trees'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-326b-vendor-bech32-kes-agent-dmq-node/
---

# Round 326b — vendor bech32 + kes-agent + dmq-node source trees

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R326`](2026-05-09-round-326-vendored-source-survey.md)  
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), prep block continuation.

## Summary

R326 closed as verification-only because the canonical IntersectMBO
source-repo URLs for `bech32`, `kes-agent`, `kes-agent-control`, and
`dmq-node` weren't documented in upstream `cardano-node`'s
`cabal.project` (these tools are consumed via cardano-haskell-packages
/ CHaP, not git submodules). External GitHub probing was sandboxed.

R326b unblocks the gap: with operator-authorized GitHub MCP plugin
reconnection, the canonical URLs were confirmed via
`mcp__plugin_github_github__search_repositories` and a directory-
listing on `input-output-hk/kes-agent`. All 3 sources cloned cleanly
under `.reference-haskell-cardano-node/deps/`.

## URLs confirmed

| Tool | Canonical URL | Default branch | `.hs` count |
|---|---|---|---:|
| bech32 | `https://github.com/IntersectMBO/bech32` | `master` | 9 |
| kes-agent (provides both `kes-agent` + `kes-agent-control` binaries) | `https://github.com/input-output-hk/kes-agent` | `master` | 68 |
| dmq-node | `https://github.com/IntersectMBO/dmq-node` | `main` | 51 |

**Total newly-vendored `.hs`:** 128 (bringing the upstream index from
4,676 to **4,804** files).

Note: `kes-agent` lives under the legacy `input-output-hk` GitHub org
(not `IntersectMBO`). Per the verified directory listing, the repo
contains both a `kes-agent/` package directory and a
`kes-agent-crypto/` companion package — sufficient to mirror both
upstream binaries.

## Diff inventory

| Path | Change |
|---|---|
| `dev/reference/setup-reference.sh` | Extended the per-repo loop from a 6-entry hardcoded `IntersectMBO/$repo.git` form to a 9-entry per-repo `dirname\|url` table that supports cross-org URLs. The 3 new entries (bech32, kes-agent, dmq-node) clone alongside the existing 6 (cardano-base, cardano-cli, cardano-ledger, ouroboros-consensus, ouroboros-network, plutus). |
| `docs/upstream-haskell-files.txt` | Refreshed: 4,676 → 4,804 entries (+128 from the 3 new repos). |
| `docs/operational-runs/2026-05-09-round-326b-vendor-bech32-kes-agent-dmq-node.md` | This round-doc. |

## Verification

```text
$ bash dev/reference/setup-reference.sh
==> materialising IntersectMBO/cardano-node 11.0.1 source
==> cloning upstream library sources into deps/
    deps/cardano-base already present, refreshing tags
    deps/cardano-cli already present, refreshing tags
    deps/cardano-ledger already present, refreshing tags
    deps/ouroboros-consensus already present, refreshing tags
    deps/ouroboros-network already present, refreshing tags
    deps/plutus already present, refreshing tags
Cloning into 'bech32'...
Cloning into 'kes-agent'...
Cloning into 'dmq-node'...
==> downloading cardano-node 11.0.1 release tarball
==> verifying SHA-256
cardano-node-11.0.1-linux-amd64.tar.gz: OK
==> extracting
==> verifying binaries run
cardano-node 11.0.1 - linux-x86_64 - ghc-9.6
=== reference setup complete (cardano-node 11.0.1) ===

$ wc -l docs/upstream-haskell-files.txt
4804 docs/upstream-haskell-files.txt

$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ python3 dev/test/check-parity-matrix.py
parity matrix clean: 8 entries validated against
    .reference-haskell-cardano-node (reference tag 11.0.1)

$ python3 dev/test/check-fixture-manifest.py
fixture manifest clean: SHA 7a8a991945d401d89e27f53b3d3bb464a354ad4c
    consistent across pin source, fixture tree, and docs;
    2 corpora validated.

$ python3 dev/test/check-reference-artifacts.py
reference artifacts clean: cardano-node 11.0.1 install +
    9 binaries + 3 network share dirs validated.
```

## Sister-tool source coverage (post-R326b — all 12 covered)

| Tool | Vendored source path | `.hs` count |
|---|---|---:|
| bech32 | `deps/bech32/` | 9 |
| cardano-cli (already mirrored at `crates/cardano-cli/`) | `deps/cardano-cli/` | 180 |
| cardano-submit-api | `cardano-submit-api/` | 14 |
| cardano-testnet | `cardano-testnet/` | 82 |
| cardano-tracer | `cardano-tracer/` | 93 |
| db-analyser (lib + app) | `deps/ouroboros-consensus/.../unstable-cardano-tools/Cardano/Tools/DBAnalyser/` + `app/db-analyser.hs` | 13 |
| db-synthesizer (lib + app) | `.../Cardano/Tools/DBSynthesizer/` + `app/db-synthesizer.hs` | 6 |
| db-truncater (lib + app) | `.../Cardano/Tools/DBTruncater/` + `app/db-truncater.hs` | 4 |
| dmq-node | `deps/dmq-node/` | 51 |
| kes-agent (incl. kes-agent-control) | `deps/kes-agent/` | 68 |
| snapshot-converter | `deps/ouroboros-consensus/.../app/snapshot-converter.hs` | 1 |
| tx-generator | `bench/tx-generator/` | 46 |

**All 12 sister tools' source is now vendored.** No further
operator action needed before Tier 1 entry (R331).

## Closure criterion

- 3 missing source trees (bech32, kes-agent, dmq-node) cloned under
  `.reference-haskell-cardano-node/deps/`.
- `setup-reference.sh` extended to a per-repo URL table; cross-org
  URLs (kes-agent under input-output-hk) supported.
- `upstream-haskell-files.txt` index refreshed to 4,804 entries.
- All 4 CI parity validators clean (strict-mirror, parity-matrix,
  fixture-manifest, reference-artifacts).

All four are met.

## Out of scope (R327+ next steps)

The prep block continues:
- **R327 — Workspace layout + Cargo skeleton stubs** for all 12 crates.
- **R328 — Audit + parity infrastructure expansion** (parity-matrix
  entries; upstream_pins.rs SHA pins for the 3 new repos; drift detector
  extension).
- **R329 — Run-tools launcher** (`node/dev/scripts/run-tools.sh`) +
  `node/configuration/preprod/checkpoints.json`.
- **R330 — Pure-Rust ecosystem dependency audit** (bech32, axum,
  tracing-appender).

Authorization checkpoint after R330 → operator approves Phase A entry.
