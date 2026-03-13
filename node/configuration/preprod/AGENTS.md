---
name: node-configuration-preprod
description: Guidance for vendored preprod configuration reference files.
---

This directory contains preprod reference configuration artifacts only.

## Scope
- Preprod `config.json`, `topology.json`, and genesis reference files.

##  Rules *Non-Negotiable*
- Do not hand-edit these reference files.
- Use them only as provenance and shape references for Yggdrasil configuration handling.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research and add or update links as needed*
- Preprod configuration reference: <https://github.com/IntersectMBO/cardano-node/tree/master/configuration/cardano/preprod/>
- Preprod environment guide: <https://book.world.dev.cardano.org/env-preprod.html>