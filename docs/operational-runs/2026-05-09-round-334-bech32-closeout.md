---
title: 'R334: bech32 closeout — first sister tool verified_11_0_1'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-334-bech32-closeout/
---

# Round 334 — bech32 closeout

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** [`R333`](2026-05-09-round-333-bech32-encode-decode.md)  
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), Phase A.1 closure (round 4 of 4).

## Summary

R334 closes Phase A.1 (bech32 mini-arc) with the canonical closeout
deliverables: AGENTS.md operational guide updated to deployment-
ready state; CHANGELOG entry under `[Unreleased]`; parity-matrix
transition from `partial` to `verified_11_0_1`.

**bech32 becomes the first sister tool with full deployment-ready
100% parity to upstream.** Operators can replace
`.reference-haskell-cardano-node/install/bin/bech32` with
`target/release/bech32` (or invoke via `node/dev/scripts/run-tools.sh
bech32`) without observing any byte-level difference in CLI surface,
encode/decode output, or error behavior.

## Diff inventory

| Path | Change |
|---|---|
| `crates/bech32/AGENTS.md` | Replaced R327 skeleton text with R334 deployment-ready operational guide. New sections: file-mirror table, build+run examples, functional surface excerpt, pure-Rust dep table with licenses, comparison-with-upstream procedure, R334 maintenance guidance. |
| `docs/parity-matrix.json` | `sister-tool.bech32` advanced: status `partial → verified_11_0_1`; next_milestone `R334 → R335` (next tool entry); 8 implemented_evidence rows; 6 acceptance-criteria checkmarks; remaining_work cleared. |
| `CHANGELOG.md` | New entry under `[Unreleased]` summarizing the R326-R334 prep + Phase A.1 closure. |
| `docs/operational-runs/2026-05-09-round-334-bech32-closeout.md` | This round-doc. |

R334 ships zero Rust code changes. The 4,887-test workspace baseline
is preserved.

## Phase A.1 cumulative scoreboard (R331-R334)

| Round | Shipped | Test delta | Parity-matrix transition |
|---|---|---:|---|
| R331 | `e35ebae` | 0 | absent → partial; next R332 |
| R332 | `71bd8fd` | +16 | partial; next R333 (CLI parser) |
| R333 | `799433d` | +15 | partial; next R334 (encode/decode + drop-in proof) |
| R334 | (this) | 0 | **partial → verified_11_0_1**; next R335 (cardano-submit-api) |
| **Total** | **4 commits** | **+31** | **First sister tool reaches `verified_11_0_1`** |

## Acceptance criteria check (per the R326-R459 plan §7)

| # | Criterion | Status |
|---|---|---|
| 1 | `bech32 --help` byte-equivalent to upstream (golden test pinned) | ✅ R332 |
| 2 | Per-subcommand byte-equivalence on documented fixtures | ✅ R333 (4 round-trip tests) |
| 3 | Drop-in deployment swap via `node/dev/scripts/run-tools.sh bech32` | ✅ R333 |
| 4 | Strict-mirror gate green (every production .rs has canonical docstring) | ✅ R331 + maintained |
| 5 | `cargo test -p yggdrasil-bech32 --test integration` green | ✅ 8 golden tests pass |
| 6 | Parity-matrix entry: `verified_11_0_1` | ✅ R334 (this round) |
| 7 | AGENTS.md operational guide | ✅ R334 (this round) |
| 8 | CHANGELOG entry | ✅ R334 (this round) |

All 8 acceptance criteria met. **bech32 is deployment-ready 100% parity.**

## Verification

```text
$ cargo fmt --all -- --check
(silent — clean)

$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)

$ cargo clippy --workspace --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.55s

$ cargo test --workspace --all-features
passed: 4887  failed: 0

$ python3 dev/test/check-parity-matrix.py
parity matrix clean: 20 entries validated; 1 sister-tool now verified_11_0_1
```

## Out of scope (Phase A.2 entry)

After R334 closeout, Phase A.1 (bech32) is complete. The next entry
is **Phase A.2 — cardano-submit-api** (R335-R343, 9 rounds, MEDIUM).

Round breakdown per the plan:
- R335 Skeleton (12-file mirror tree)
- R336 CLI parser (--config, --mainnet/--testnet-magic, --socket-path, --port, --metrics-port)
- R337 Types + Orphans + Util ports
- R338 Rest/{Types, Parsers, Web} ports
- R339 Tracing/TraceSubmitApi port
- R340 Web.hs HTTP server port (axum dep added; 1st HTTP-framework decision in the arc)
- R341 Metrics.hs Prometheus port
- R342 Integration via `crates/network/src/local_tx_submission_client.rs`
- R343 Closeout

Phase A.2 closure brings the 2nd sister tool to `verified_11_0_1`
status.
