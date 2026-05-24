---
title: Parity Dashboard
layout: default
parent: Reference
nav_order: 2
---

# Parity Dashboard

Compact status board sourced from [`docs/parity-matrix.json`](parity-matrix.json) and the living parity/operator docs.

_Last updated: 2026-05-24._

| Metric | Current value | Detail link |
| --- | ---: | --- |
| Total parity entries | 22 | [`PARITY_SUMMARY.md` → Current Implementation Status](PARITY_SUMMARY.md#current-implementation-status-1-sentence-per-subsystem) |
| `verified_11_0_1` | 2 | [`UPSTREAM_PARITY.md` → Subsystem Status](UPSTREAM_PARITY.md#subsystem-status) |
| `implemented_needs_11_0_1_evidence` | 12 | [`UPSTREAM_PARITY.md` → Verification Baseline](UPSTREAM_PARITY.md#verification-baseline) |
| `partial` | 8 | [`COMPLETION_ROADMAP.md` → Category A](COMPLETION_ROADMAP.md#category-a--executable-now-no-external-dependency) |
| Open blocking gaps | Gap BO, Gap BP, R178-followup (Conway HFC LSQ envelope) | [`UPSTREAM_PARITY.md` → Open Gaps](UPSTREAM_PARITY.md#open-gaps) |
| Required operator gates remaining | Mainnet endurance rehearsal (§2–9 runbook) and §6.5 parallel BlockFetch sign-off | [`COMPLETION_ROADMAP.md` → Category B](COMPLETION_ROADMAP.md#category-b--operator-soak-gated) |

## Notes

- First-pass counts are taken directly from the current `parity-matrix.json` entry statuses.
- This page is intended as a compact index; details and evidence remain in the linked source docs.
