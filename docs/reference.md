---
title: Reference
layout: default
nav_order: 3
has_children: true
permalink: /reference/
---

# Reference

Technical reference material for developers and auditors. The user manual covers operational concerns; this section covers the architecture, parity status, and contributing process.

## Contents

- [Architecture]({{ "/ARCHITECTURE/" | relative_url }}) — workspace structure, dependency direction, mini-protocol layering, Phase 6 multi-peer dispatch.
- [Parity Plan]({{ "/PARITY_PLAN/" | relative_url }}) — exhaustive subsystem-by-subsystem comparison against upstream Haskell.
- [Parity Summary]({{ "/PARITY_SUMMARY/" | relative_url }}) — high-level parity status for management / non-engineering audiences.
- [Audit Verification 2026-Q2]({{ "/AUDIT_VERIFICATION_2026Q2/" | relative_url }}) — the audit document driving the closure cycle, with per-slice closure-status table.
- [Upstream Parity Matrix]({{ "/UPSTREAM_PARITY/" | relative_url }}) — pinned IntersectMBO commit SHAs and subsystem references.
- [Specifications]({{ "/SPECS/" | relative_url }}) — pinned CDDL fixtures and their provenance.
- [Dependencies]({{ "/DEPENDENCIES/" | relative_url }}) — third-party crate audit.
- [Manual Test Runbook]({{ "/MANUAL_TEST_RUNBOOK/" | relative_url }}) — operator wallclock validation procedures (preprod/mainnet sync, hash-compare vs Haskell, restart resilience, parallel-fetch §6.5).
- [Real Preprod Pool Verification]({{ "/REAL_PREPROD_POOL_VERIFICATION/" | relative_url }}) — preprod block-production rehearsal record.
- [Upstream Research]({{ "/UPSTREAM_RESEARCH/" | relative_url }}) — research notes on the Haskell node subsystems.
- [Contributing]({{ "/CONTRIBUTING/" | relative_url }}) — coding style, AGENTS.md operational rules, PR workflow.
