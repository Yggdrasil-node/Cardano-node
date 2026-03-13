---
name: consensus-src
description: Guidance for consensus source modules implementing typed chain selection and nonce evolution logic.
---

This directory owns consensus implementation modules, not integration glue.

## Scope
- `chain_state`, `nonce`, `header`, `leader`, and operational certificate logic.
- Typed consensus math and verification rules.

##  Rules *Non-Negotiable*
- Consensus math and rollback rules MUST stay explicit and typed.
- Source modules here MUST remain independent of node runtime orchestration concerns.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research and add or update links as needed*
- Consensus source tree: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/>
- Cardano consensus integration: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus-cardano/src/>
- Consensus Agda specification: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/docs/agda-spec/>

## Current Phase
- Preserve the current separation between header verification, epoch nonce evolution, and volatile chain tracking.