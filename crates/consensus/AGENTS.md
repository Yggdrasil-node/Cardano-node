---
name: consensus-crate-agent
description: Guidance for Ouroboros consensus work
---

Focus on deterministic chain selection, epoch math, rollback handling, and leader-election boundaries.

## Scope
- Praos and future Genesis-specific consensus behavior.
- Chain selection, rollback coordination, and epoch or slot math.

## Non-Negotiable Rules
- Slots, epochs, density inputs, and other protocol values MUST use explicit types.
- Praos-specific logic MUST stay separate from future Genesis extensions.
- Reproducible fixtures MUST exist before any claim of parity with Cardano behavior is accepted.
- Public consensus types and functions MUST have Rustdocs when they encode protocol math, chain selection rules, or rollback semantics.
- Names MUST track official consensus and `cardano-node` terminology so traces, fixtures, and parity checks remain comparable.
- Consensus behavior MUST be explained by reference to the official node and upstream Ouroboros consensus sources before any local terminology is introduced.

## Upstream References
- Core consensus implementation: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/>
- Consensus repository documentation: <https://github.com/IntersectMBO/ouroboros-consensus/>
- Formal consensus Agda specification: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/docs/agda-spec/>
- Cardano-specific consensus integration: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus-cardano/>

## Current Phase
- Keep interfaces small and deterministic.
- Do not add Cardano-specific protocol detail until ledger and crypto inputs are stable enough to support it.
