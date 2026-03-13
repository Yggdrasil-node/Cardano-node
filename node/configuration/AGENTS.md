---
name: node-configuration
description: Guidance for node configuration references and preset layout.
---

This directory contains reference configuration files and per-network presets used to align Yggdrasil configuration handling with the official node.

## Scope
- `mainnet/`, `preprod/`, and `preview/` preset directories.
- Reference `config.json`, `topology.json`, and genesis files used to mirror official network layout.

## Non-Negotiable Rules
- Treat the vendored configuration files here as reference inputs, not as the source of truth for local runtime configuration code.
- Preserve the official file naming and preset split so parity work stays traceable.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References (add or update as needed)
- Official configuration tree: <https://github.com/IntersectMBO/cardano-node/tree/master/configuration/>
- Node runtime configuration usage: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-node/>
- Mainnet environment reference: <https://book.world.dev.cardano.org/env-mainnet.html>
- Preprod environment reference: <https://book.world.dev.cardano.org/env-preprod.html>
- Preview environment reference: <https://book.world.dev.cardano.org/env-preview.html>

## Current Phase
- Yggdrasil exposes `NetworkPreset` values for `Mainnet`, `Preprod`, and `Preview` and keeps reference files here for shape and provenance rather than direct in-place editing.