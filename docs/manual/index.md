---
title: User Manual
layout: default
nav_order: 2
has_children: true
permalink: /manual/
---

# User Manual

This manual walks an operator through every stage of running a Yggdrasil Cardano node, from a fresh machine to a long-running production deployment.

## How to read this manual

The chapters are ordered so each one builds on the previous. If you are starting from scratch, read them in sequence:

1. **[Overview]({{ "/manual/overview/" | relative_url }})** sets the conceptual frame: what a Cardano node does and what role Yggdrasil plays.
2. **[Installation]({{ "/manual/installation/" | relative_url }})** gets the binary onto the machine.
3. **[Quick Start]({{ "/manual/quick-start/" | relative_url }})** syncs mainnet with default settings — the fastest path to a working node.
4. **[Networks and Presets]({{ "/manual/networks/" | relative_url }})** explains mainnet vs. preprod vs. preview and how presets work.
5. **[Configuration]({{ "/manual/configuration/" | relative_url }})** is the reference for every config key, CLI flag, and topology option.
6. **[Running a Node]({{ "/manual/running/" | relative_url }})** covers daemonisation, signal handling, and graceful shutdown.
7. **[Monitoring]({{ "/manual/monitoring/" | relative_url }})** wires the node into Prometheus and structured logging.
8. **[Block Production]({{ "/manual/block-production/" | relative_url }})** is the additional setup needed for a stake pool operator.
9. **[CLI Reference]({{ "/manual/cli-reference/" | relative_url }})** documents every subcommand and flag of the `yggdrasil-node` binary.
10. **[Maintenance]({{ "/manual/maintenance/" | relative_url }})** covers backups, garbage collection, upgrades, and log rotation.
11. **[Troubleshooting]({{ "/manual/troubleshooting/" | relative_url }})** lists common error messages and their resolutions.
12. **[Glossary]({{ "/manual/glossary/" | relative_url }})** defines Cardano-specific terminology.

## Conventions

- Commands prefixed with `$` are run as a non-root user.
- Commands prefixed with `#` are run as root.
- Paths in `<angle brackets>` are placeholders you replace with your actual paths.
- Code blocks without a prompt are file contents.
- "Upstream" refers to the Haskell `cardano-node` from IntersectMBO, which Yggdrasil targets for parity.

## Operating-system support

| OS                  | Build | Run | Notes                                        |
|---------------------|-------|-----|----------------------------------------------|
| Linux x86_64        | yes   | yes | Primary supported platform                   |
| Linux aarch64       | yes   | yes | ARM64 servers, Raspberry Pi 4/5 64-bit       |
| macOS (Apple Silicon)| yes  | yes | For development; not recommended for mainnet pools |
| macOS (Intel)       | yes   | yes | Same as above                                |
| Windows             | partial| no | Some Unix-only features (NtC LocalStateQuery, LocalTxSubmission) are gated behind `cfg(unix)` |

A production stake pool should run on Linux on a server-class CPU with at least 16 GB RAM, 500 GB SSD, and a stable network connection.
