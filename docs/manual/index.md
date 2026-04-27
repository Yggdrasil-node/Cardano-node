---
title: User Manual
layout: default
nav_order: 2
has_children: true
permalink: /manual/
description: "Operator manual for the Yggdrasil Cardano node — install, configure, run, monitor, produce blocks, and maintain over time."
---

<div class="yg-hero" markdown="0">
  <span class="yg-eyebrow">Operator manual</span>
  <h1>Run a Yggdrasil node, end-to-end.</h1>
  <p class="yg-lead">
    Fourteen chapters that walk an operator from a fresh machine to a
    long-running production deployment. Start at the top if you're new;
    use the cards below to jump straight to what you need.
  </p>
  <div class="yg-hero-actions">
    <a class="yg-btn yg-btn-primary" href="{{ '/manual/quick-start/' | relative_url }}">Quick Start →</a>
    <a class="yg-btn yg-btn-secondary" href="{{ '/manual/installation/' | relative_url }}">Installation</a>
  </div>
</div>

## Get a node running

<ul class="yg-cards" markdown="0">
  <li>
    <a class="yg-card" href="{{ '/manual/overview/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">1. Overview</span>
      <span class="yg-card-desc">What a Cardano node does and where Yggdrasil fits.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/manual/installation/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">2. Installation</span>
      <span class="yg-card-desc">Prerequisites and a clean build from source.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/manual/releases/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">3. From releases</span>
      <span class="yg-card-desc">Pre-built Linux binaries with SHA256 verification.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/manual/quick-start/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">4. Quick Start</span>
      <span class="yg-card-desc">Sync mainnet in five commands.</span>
    </a>
  </li>
</ul>

## Configure for your network

<ul class="yg-cards" markdown="0">
  <li>
    <a class="yg-card" href="{{ '/manual/networks/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">5. Networks &amp; presets</span>
      <span class="yg-card-desc">Mainnet, preprod, preview — when and why each.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/manual/configuration/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">6. Configuration</span>
      <span class="yg-card-desc">Every config key, CLI flag, and topology option.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/manual/running/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">7. Running a node</span>
      <span class="yg-card-desc">Daemonising, signal handling, graceful shutdown.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/manual/docker/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">8. Docker</span>
      <span class="yg-card-desc">Containerised deployment with <code>docker compose</code>.</span>
    </a>
  </li>
</ul>

## Operate over time

<ul class="yg-cards" markdown="0">
  <li>
    <a class="yg-card" href="{{ '/manual/monitoring/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">9. Monitoring</span>
      <span class="yg-card-desc">Prometheus, structured traces, dashboards, alerts.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/manual/block-production/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">10. Block production</span>
      <span class="yg-card-desc">KES, VRF, OpCert — stake pool credentials.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/manual/cli-reference/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">11. CLI reference</span>
      <span class="yg-card-desc">Every <code>yggdrasil-node</code> subcommand and flag.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/manual/maintenance/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">12. Maintenance</span>
      <span class="yg-card-desc">Backups, KES rotation, upgrades, log rotation.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/manual/troubleshooting/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">13. Troubleshooting</span>
      <span class="yg-card-desc">Common errors and the resolutions that work.</span>
    </a>
  </li>
  <li>
    <a class="yg-card" href="{{ '/manual/glossary/' | relative_url }}">
      <span class="yg-card-arrow">→</span>
      <span class="yg-card-title">14. Glossary</span>
      <span class="yg-card-desc">Cardano terminology, defined.</span>
    </a>
  </li>
</ul>

## Conventions

- Commands prefixed with `$` are run as a non-root user.
- Commands prefixed with `#` are run as root.
- Paths in `<angle brackets>` are placeholders you replace with your actual paths.
- Code blocks without a prompt are file contents.
- "Upstream" refers to the Haskell `cardano-node` from IntersectMBO, which Yggdrasil targets for parity.

## Operating-system support

| OS                   | Build | Run | Notes |
|----------------------|:-----:|:---:|-------|
| Linux x86_64         | ✓ | ✓ | Primary supported platform |
| Linux aarch64        | ✓ | ✓ | ARM64 servers, Raspberry Pi 4/5 64-bit |
| macOS (Apple Silicon)| ✓ | ✓ | Development; not recommended for mainnet pools |
| macOS (Intel)        | ✓ | ✓ | Development; not recommended for mainnet pools |
| Windows              | partial | — | Some Unix-only features (`query`, `submit-tx`) gated behind `cfg(unix)` |

A production stake pool should run on Linux on a server-class CPU with at least 16 GB RAM, 500 GB SSD, and a stable network connection.
