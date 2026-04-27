---
title: Home
layout: default
nav_order: 1
---

# Yggdrasil

A pure-Rust implementation of the Cardano node, targeting long-term protocol and serialization parity with the upstream Haskell node ([IntersectMBO](https://github.com/IntersectMBO)).

This site is the operator manual: how to install, configure, run, and maintain a Yggdrasil node.

## Quick links

- **New operator?** Start with the [User Manual]({{ "/manual/" | relative_url }}).
- **Want to sync mainnet now?** Jump to [Quick Start]({{ "/manual/quick-start/" | relative_url }}).
- **Running a stake pool?** See [Block Production]({{ "/manual/block-production/" | relative_url }}).
- **Hit an error?** Check [Troubleshooting]({{ "/manual/troubleshooting/" | relative_url }}).

## Project status

Yggdrasil 1.0 is feature-complete against the 2026-Q2 parity audit. All ledger eras Byron through Conway are wired through ledger, storage, consensus, mempool, network, Plutus, and the node binary. **4,630 tests** pass across the workspace. The remaining production-readiness gate is operator-side wallclock validation per the [Manual Test Runbook]({{ "/MANUAL_TEST_RUNBOOK/" | relative_url }}).

For the technical architecture and parity status see:

- [Architecture]({{ "/ARCHITECTURE/" | relative_url }})
- [Parity Summary]({{ "/PARITY_SUMMARY/" | relative_url }})
- [Parity Plan]({{ "/PARITY_PLAN/" | relative_url }})
- [Audit Verification]({{ "/AUDIT_VERIFICATION_2026Q2/" | relative_url }})

## Manual contents

The user manual is organised as a sequence — each chapter assumes the previous ones:

1. [Overview]({{ "/manual/overview/" | relative_url }}) — what Yggdrasil is and how it fits into the Cardano network
2. [Installation]({{ "/manual/installation/" | relative_url }}) — prerequisites, building from source, verifying the binary
3. [Quick Start]({{ "/manual/quick-start/" | relative_url }}) — sync a node to mainnet in five commands
4. [Networks and Presets]({{ "/manual/networks/" | relative_url }}) — mainnet, preprod, preview
5. [Configuration]({{ "/manual/configuration/" | relative_url }}) — every config key, CLI override, and topology option
6. [Running a Node]({{ "/manual/running/" | relative_url }}) — daemonising, log management, graceful shutdown
7. [Monitoring]({{ "/manual/monitoring/" | relative_url }}) — Prometheus, structured tracing, dashboards
8. [Block Production]({{ "/manual/block-production/" | relative_url }}) — KES/VRF/OpCert credentials for stake pool operators
9. [CLI Reference]({{ "/manual/cli-reference/" | relative_url }}) — every subcommand and flag
10. [Maintenance]({{ "/manual/maintenance/" | relative_url }}) — backups, garbage collection, upgrades, log rotation
11. [Troubleshooting]({{ "/manual/troubleshooting/" | relative_url }}) — common errors and their resolutions
12. [Glossary]({{ "/manual/glossary/" | relative_url }}) — Cardano terminology

## Getting help

- File an issue at the GitHub repository
- For protocol-level questions, consult the [Cardano Operations Book](https://book.world.dev.cardano.org/)
- For Cardano network status, see [Cardano Explorer](https://explorer.cardano.org/) or [Pool.pm](https://pool.pm/)
