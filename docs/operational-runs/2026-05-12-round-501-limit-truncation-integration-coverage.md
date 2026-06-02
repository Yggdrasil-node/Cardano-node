---
title: 'R501: Integration coverage for Limit::Limit(n) truncation in db-analyser'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-12-round-501-limit-truncation-integration-coverage/
---

# R501 — Limit truncation integration coverage

**Date:** 2026-05-12
**Predecessor:** R500 (ledger-state-dependent handlers
integration suite).
**Scope:** tests-only round.

## Slice scope

R479's `apply_limit` helper implements `Limit::Limit(n)`
truncation via `blocks.into_iter().take(n).collect()`. The R479
unit test
`run_analysis_respects_conf_limit` exercises this with
`Vec<Block>` input. But all 11 integration tests in
`tests/end_to_end_chain_walk.rs` (R481 + R500) use
`Limit::Unlimited` — none exercise the truncation path
end-to-end via FileImmutable.

R501 closes that gap. 3 new integration tests:

| Test | Coverage |
|------|----------|
| `end_to_end_count_blocks_respects_limit_truncation` | 5-block FileImmutable + `Limit::Limit(2)` → asserts `total=2`, first/last reflect blocks 1-2 |
| `end_to_end_show_slot_block_no_respects_limit_truncation` | 3-block FileImmutable + `Limit::Limit(1)` → asserts exactly 1 row |
| `end_to_end_limit_unlimited_is_equivalent_to_no_truncation` | Invariant: `Limit::Unlimited` and `Limit::Limit(N)` where `N >= chain.len()` yield identical outcomes |

A new `mk_config_with_limit` helper parametrizes the limit field
(was hard-coded to `Limit::Unlimited` in `mk_config`).

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,229 → 6,232
python3 dev/test/check-strict-mirror.py --fail-on-violation   0 violations
python3 dev/test/check-parity-matrix.py                       clean
```

## Stop point

Integration coverage at HEAD: 14 tests total (was 11 at R500
closeout). Every `AnalysisName` variant has at least one
end-to-end FileImmutable integration test; `Limit::Limit(n)`
truncation path now also covered end-to-end.
