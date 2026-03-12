---
name: consensus-crate-agent
description: Guidance for Ouroboros consensus work
---

Focus on deterministic chain selection, epoch math, rollback handling, and leader-election boundaries.

## Scope
- Praos and future Genesis-specific consensus behavior.
- Chain selection, rollback coordination, and epoch or slot math.

## Rules
- Prefer explicit types for slots, epochs, and density inputs.
- Keep Praos-specific logic separate from future Genesis extensions.
- Require reproducible fixtures before claiming parity with Cardano behavior.

## Upstream References
- Core consensus implementation: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus>
- Consensus repository documentation: <https://github.com/IntersectMBO/ouroboros-consensus>
- Formal consensus Agda specification: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/docs/agda-spec>
- Cardano-specific consensus integration: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus-cardano>

## Current Phase
- Keep interfaces small and deterministic.
- Do not add Cardano-specific protocol detail until ledger and crypto inputs are stable enough to support it.
