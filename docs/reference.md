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
    <a class="yg-card" href="{{ '/PARITY_PLAN/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Parity plan</span>
      <span class="yg-card-desc">Exhaustive subsystem-by-subsystem comparison against upstream Haskell.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/PARITY_SUMMARY/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Parity summary</span>
      <span class="yg-card-desc">High-level parity status for management and non-engineering audiences.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/AUDIT_VERIFICATION_2026Q2/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">2026-Q2 audit</span>
      <span class="yg-card-desc">The audit document driving the closure cycle, with the per-slice closure-status table.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/UPSTREAM_PARITY/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Upstream parity matrix</span>
      <span class="yg-card-desc">Pinned IntersectMBO commit SHAs and subsystem references.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/UPSTREAM_RESEARCH/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Upstream research</span>
      <span class="yg-card-desc">Research notes on the Haskell node subsystems consumed during the port.</span>
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
