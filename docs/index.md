---
title: Home
layout: default
nav_order: 1
description: "Yggdrasil — a pure-Rust Cardano node. Operator manual: install, configure, run, monitor, and produce blocks."
---

<figure class="yg-hero-banner" markdown="0">
  <img src="{{ '/assets/images/Yggrasil_banner.png' | relative_url }}"
       alt="YggdrasilNode — A Cardano Node Project Written In Rust"
       loading="eager"
       decoding="async">
</figure>

<div class="yg-hero" markdown="0">
  <span class="yg-eyebrow">Yggdrasil v0.1.0 · Pure Rust</span>
  <h1>A pure-Rust Cardano node, built for parity.</h1>
  <p class="yg-lead">
    Yggdrasil is a from-scratch Rust implementation of the Cardano node,
    targeting long-term protocol and serialization parity with the upstream
    Haskell <a href="https://github.com/IntersectMBO">IntersectMBO</a> node.
    No FFI cryptography. No hidden native dependencies. Just Rust.
  </p>
  <div class="yg-hero-actions">
    <a class="yg-btn yg-btn-primary" href="{{ '/manual/quick-start/' | relative_url }}">Quick Start →</a>
    <a class="yg-btn yg-btn-secondary" href="{{ '/manual/installation/' | relative_url }}">Install from source</a>
    <a class="yg-btn yg-btn-secondary" href="https://github.com/yggdrasil-node/Cardano-node">GitHub</a>
  </div>
</div>

<div class="yg-stats" markdown="0">
  <div class="yg-stat yg-stat-accent"><span class="yg-stat-value">4.7K+</span><span class="yg-stat-label">Tests passing</span></div>
  <div class="yg-stat"><span class="yg-stat-value">0</span><span class="yg-stat-label">Tests failing</span></div>
  <div class="yg-stat"><span class="yg-stat-value">Byron→Conway</span><span class="yg-stat-label">Era coverage</span></div>
  <div class="yg-stat"><span class="yg-stat-value">5 / 5</span><span class="yg-stat-label">Mini-protocols</span></div>
  <div class="yg-stat"><span class="yg-stat-value">v0.1.0</span><span class="yg-stat-label">Latest release</span></div>
</div>

## Where to start

<ul class="yg-cards" markdown="0">
  <li>
    <a class="yg-card" href="{{ '/manual/quick-start/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Quick Start</span>
      <span class="yg-card-desc">Sync a mainnet relay in five commands. The fastest path from a fresh machine to a working node.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/manual/installation/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Installation</span>
      <span class="yg-card-desc">Prerequisites, build from source, sanity-check the binary, install system-wide.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/manual/docker/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Docker</span>
      <span class="yg-card-desc">Run as a container with <code>docker compose</code> — relay or block producer.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/manual/block-production/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Block Production</span>
      <span class="yg-card-desc">KES, VRF, and OpCert credential setup for stake pool operators.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/manual/monitoring/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Monitoring</span>
      <span class="yg-card-desc">Prometheus metrics, structured tracing, suggested alerts, Grafana dashboards.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/manual/troubleshooting/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Troubleshooting</span>
      <span class="yg-card-desc">Common errors, their causes, and the resolutions that work.</span>
    </a>
  </li>
</ul>

## What's implemented

Every confirmed-active code-level parity slice from the [2026-Q2 audit]({{ "/AUDIT_VERIFICATION_2026Q2/" | relative_url }}) is closed, including all runtime integrations originally tracked as follow-ups and the R238 rollback sidecar hardening work.

| Subsystem | Status |
|-----------|--------|
| **Crypto** — Blake2b, Ed25519, VRF (std + batchcompat), KES (Simple + Sum 0–6+), BLS12-381, secp256k1 | Complete |
| **Ledger** — eras Byron through Conway, multi-era UTxO, governance, PPUP, MIR, ratification | Complete |
| **Storage** — file-backed `ImmutableStore` / `VolatileStore` / `LedgerStore` + `ChainDb` + ChainDepState sidecars | Complete |
| **Consensus** — Praos leader election, KES/OpCert, `ChainState`, nonce evolution | Complete |
| **Mempool** — fee-ordered queue, TTL admission, ledger revalidation, eviction | Complete |
| **Network** — mux, all 5 mini-protocols, governor, ledger peers, diffusion types | Complete |
| **Plutus** — CEK machine, builtins, calibrated cost model | Complete |
| **Node binary** — sync runtime, inbound server, NtC, block production | Complete |

## Reference material

<ul class="yg-cards" markdown="0">
  <li>
    <a class="yg-card" href="{{ '/ARCHITECTURE/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Architecture</span>
      <span class="yg-card-desc">Workspace layout, dependency direction, mini-protocol layering.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/PARITY_SUMMARY/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Parity summary</span>
      <span class="yg-card-desc">High-level parity status against upstream IntersectMBO.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/MANUAL_TEST_RUNBOOK/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Manual test runbook</span>
      <span class="yg-card-desc">Operator wallclock validation: preprod/mainnet sync, hash compare, restart resilience.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/CHANGELOG/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">Changelog</span>
      <span class="yg-card-desc">Release-by-release record of what changed.</span>
    </a>
  </li>
</ul>

## Getting help

- File an issue on [GitHub](https://github.com/yggdrasil-node/Cardano-node/issues).
- For protocol-level questions, consult the [Cardano Operations Book](https://book.world.dev.cardano.org/).
- For network status, see [Cardano Explorer](https://explorer.cardano.org/) or [Pool.pm](https://pool.pm/).
