---
title: Reference
layout: default
nav_order: 3
has_children: true
permalink: /reference/
description: "Architecture, parity, and contributing references for developers and auditors."
---

<div class="yg-hero" markdown="0">
  <span class="yg-eyebrow">Technical reference</span>
  <h1>Architecture, parity, and contribution.</h1>
  <p class="yg-lead">
    Material for engineers and auditors evaluating the Yggdrasil
    implementation against the upstream Haskell node — not for operators
    running a node. For installation and operation, see the
    <a href="{{ "/manual/" | relative_url }}">User Manual</a>.
  </p>
</div>

## Architecture &amp; parity

<ul class="yg-cards" markdown="0">
  <li>
    <a class="yg-card" href="{{ '/ARCHITECTURE/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Architecture</span>
      <span class="yg-card-desc">Workspace structure, dependency direction, mini-protocol layering, multi-peer dispatch, rollback sidecars.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/PARITY_PROOF/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Parity proof</span>
      <span class="yg-card-desc">Operational verification reference: 25/25 cardano-cli LSQ subcommands, consensus sidecar persistence, remaining gaps, runbook.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/PARITY_SUMMARY/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Parity summary</span>
      <span class="yg-card-desc">Subsystem status table + per-function inventory + cumulative round audit history (management + stakeholder facing).</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/UPSTREAM_PARITY/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Upstream parity matrix</span>
      <span class="yg-card-desc">Pinned IntersectMBO commit SHAs, subsystem-reference table, drift snapshot, open gaps.</span>
    </a>
  </li>
</ul>

### Archived planning docs

These documents drove the project's planning + audit cycles before
the R273-rename + Phase A–F (R274–R301) execution arc shipped. They
are preserved as audit-trail evidence; for current state, follow the
live links above.

<ul class="yg-cards" markdown="0">
  <li>
    <a class="yg-card" href="{{ '/PARITY_PLAN/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Parity plan (2026-03 archive)</span>
      <span class="yg-card-desc">Original 2026-03-26 pre-execution planning doc. Phases A–F shipped via R274–R298 — see live successors above.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/AUDIT_VERIFICATION_2026Q2/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">2026-Q2 audit (archive)</span>
      <span class="yg-card-desc">2026-Q2 sanity audit. R287 confirmed every C-1/H-1/H-2/M-1..M-8/L-1..L-9 finding closed in the 2026-Q3 operational pass.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/UPSTREAM_RESEARCH/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Upstream research (archive)</span>
      <span class="yg-card-desc">Pre-plan Haskell node research (2026-05-01). Superseded by Architecture, Parity proof, and the per-file strict-mirror audit TSV.</span>
    </a>
  </li>
</ul>

## Specs &amp; dependencies

<ul class="yg-cards" markdown="0">
  <li>
    <a class="yg-card" href="{{ '/SPECS/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Specifications</span>
      <span class="yg-card-desc">Pinned CDDL fixtures and their provenance.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/DEPENDENCIES/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Dependencies</span>
      <span class="yg-card-desc">Third-party crate audit and the rules for adding new ones.</span>
    </a>
  </li>
</ul>

## Validation &amp; release

<ul class="yg-cards" markdown="0">
  <li>
    <a class="yg-card" href="{{ '/MANUAL_TEST_RUNBOOK/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Manual test runbook</span>
      <span class="yg-card-desc">Operator wallclock validation: preprod/mainnet sync, hash compare, restart resilience, parallel-fetch §6.5.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/REAL_PREPROD_POOL_VERIFICATION/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Preprod pool verification</span>
      <span class="yg-card-desc">Preprod block-production rehearsal record.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/CONTRIBUTING/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Contributing</span>
      <span class="yg-card-desc">Coding style, AGENTS.md operational rules, PR workflow.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/CHANGELOG/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Changelog</span>
      <span class="yg-card-desc">Release notes and the release process.</span>
    </a>
  </li>
</ul>
