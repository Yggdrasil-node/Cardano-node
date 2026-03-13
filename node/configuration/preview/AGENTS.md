---
name: node-configuration-preview
description: Guidance for vendored preview configuration reference files.
---

This directory contains preview reference configuration artifacts only.

## Scope
- Preview `config.json`, `topology.json`, and genesis reference files.

## Non-Negotiable Rules
- Do not hand-edit these reference files.
- Use them only as provenance and shape references for Yggdrasil configuration handling.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References (add or update as needed)
- Preview configuration reference: <https://github.com/IntersectMBO/cardano-node/tree/master/configuration/cardano/preview/>
- Preview environment guide: <https://book.world.dev.cardano.org/env-preview.html>