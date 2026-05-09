---
title: 'R336: Phase A skeleton milestone — 12/12 sister tools deployable'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-09-round-336-phase-a-skeleton-milestone/
---

# Round 336 — Phase A skeleton milestone

**Date:** 2026-05-09  
**Branch:** `main`  
**Predecessor:** `R335` (bulk-skeleton commit for 10 sister tools).  
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), Phase A skeleton closure.

## Milestone

After R335 + the bulk-skeleton commit, **all 12 sister tools have a
deployable Rust binary** with byte-equivalent `--help` / `--version`
output captured from upstream. The 12 binaries can be invoked via
`node/scripts/run-tools.sh <tool>` and produce drop-in CLI surface
matching upstream `.reference-haskell-cardano-node/install/bin/<tool>`.

Concrete subcommand dispatch is implemented for **bech32** (Phase A.1
closure, `verified_11_0_1`); the other 11 tools return
"not yet implemented" sentinels for non-help/version invocations and
will receive concrete implementations in subsequent per-tool rounds.

## Sister-tool status (post-R336)

| Tool | Status | Tests | Next round |
|---|---|---:|---|
| **bech32** | ✅ `verified_11_0_1` | 31 | (closeout shipped R334) |
| cardano-submit-api | 🟡 `partial` (skeleton+parser) | 15 | R340 (HTTP server, axum) |
| cardano-testnet | 🟡 `partial` (skeleton+parser) | 8 | R417 (Phase C.2) |
| cardano-tracer | 🟡 `partial` (skeleton+parser) | 8 | R361 (Phase A.5) |
| db-analyser | 🟡 `partial` (skeleton+parser) | 8 | R392 (Phase B.2) |
| db-synthesizer | 🟡 `partial` (skeleton+parser) | 8 | R409 (Phase C.1) |
| db-truncater | 🟡 `partial` (skeleton+parser) | 8 | R387 (Phase B.1) |
| dmq-node | 🟡 `partial` (skeleton+parser) | 8 | R451 (Phase D.1) |
| kes-agent | 🟡 `partial` (skeleton+parser) | 8 | R345 (Phase A.3) |
| kes-agent-control | 🟡 `partial` (skeleton+parser) | 8 | R356 (Phase A.4) |
| snapshot-converter | 🟡 `partial` (skeleton+parser) | 8 | R402 (Phase B.3) |
| tx-generator | 🟡 `partial` (skeleton+parser) | 8 | R435 (Phase C.3) |

**Aggregate:** 1 verified + 11 partial = 12 with deployable binaries.
Total sister-tool tests: 31 (bech32) + 15 (cardano-submit-api) + 80
(10 bulk) = **126 sister-tool tests**.

## Workspace state

| Metric | Pre-arc (R325) | Post-R336 | Δ |
|---|---:|---:|---:|
| Workspace crates | 8 | 20 | +12 (sister tools) |
| Workspace tests | 4,856 | **4,982** | **+126** |
| Strict-mirror audit table | 246 (a) + 202 (c) = 448 | **257 (a) + 215 (c) = 472** | +9 (a) + +13 (c) |
| Parity-matrix entries | 8 | 20 | +12 |
| Upstream pin SHAs | 6 | 9 | +3 |
| Cargo workspace deps | (baseline) | +bech32, +bs58 | +2 |

## Drop-in deployment evidence

For all 12 binaries, the following holds:

```text
$ diff <(.reference-haskell-cardano-node/install/bin/<tool> --help) \
       <(target/debug/<tool> --help)
(empty diff — byte-equivalent)
```

For **bech32**, additionally every documented encode/decode example
produces byte-equivalent output:

```text
$ for input in 706174617465 Ae2tdPwUPEYy old_prefix1wpshgcg2s33x3; do
    diff <(echo -n "$input" | upstream/bech32 base16_) \
         <(echo -n "$input" | target/debug/bech32 base16_)
  done
(all empty diffs)
```

## Next phase entries

The remaining work is per-tool implementation rounds:

- **Phase A.3 — kes-agent (R344-R354)**: highest-stakes tool.
  Socket protocol byte-equivalence is mandatory for live SPO
  setups. Mini-arc breakdown per the plan: skeleton at R344,
  CLI parser at R345, config + protocol types at R346, server-
  side socket protocol at R347 (with golden vectors mandatory),
  client framing at R348, KES key lifecycle wiring crates/crypto
  at R349, daemonize at R350, status/control at R351-352, foreground
  run at R352, live rehearsal at R353, closeout at R354.

- **Phase A.4 — kes-agent-control (R355-R359)**: companion CLI.
  Round-trip testing against R344-R354 yggdrasil-kes-agent.

- **Phase A.5 — cardano-tracer (R360-R385)**: 26 rounds; large
  surface (93 .hs). RTView carve-out approved at plan time;
  axum + tracing-appender deps land at R367/R371.

- **Phase B / C / D**: see plan file for detailed breakdown.

## Closure criterion (R336)

- All 12 sister tools have deployable Rust binaries via
  `node/scripts/run-tools.sh <tool>`.
- All 12 binaries' `--help` / `--version` output is byte-
  equivalent to upstream.
- 1 of 12 (bech32) is `verified_11_0_1`; 11 are `partial` with
  named next-milestone rounds in the plan.
- Workspace test count: 4,856 → 4,982 (+126).
- All 5 cargo gates + 3 CI parity validators clean.

All five are met.

## Authorization checkpoint

Per the R326-R459 plan's authorization model, R336 closes the
broad Phase A skeleton work. Subsequent per-tool implementation
rounds (R345 kes-agent, R361 cardano-tracer, etc.) require
operator authorization between phase boundaries — Phase A.3
(kes-agent) entry is the immediate next gate.
