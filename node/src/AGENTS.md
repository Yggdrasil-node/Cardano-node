---
name: node-src-subagent
description: Guidance for node runtime and sync orchestration implementation details
---

Focus on runtime composition of network clients and orchestration helpers that remain thin integration layers.

## Scope
- `runtime.rs`, `sync.rs`, and library exports under `node/src`.
- Peer bootstrap wiring and sync control flow coordination.

## Non-Negotiable Rules
- Keep ledger and consensus business rules outside `node/src`.
- Favor small, explicit orchestration steps that are easy to smoke-test.
- Propagate typed errors and avoid hidden retries/backoff logic unless explicitly required.
- Keep naming close to `cardano-node` operational concepts.
- Public orchestration helpers MUST include Rustdocs when flow is non-trivial.

## Upstream References (add or update as needed)
- `cardano-node` runtime: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-node>
- `ouroboros-consensus` integration behavior: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus>

## Current Phase
- Bootstrap wiring is implemented (`NodeConfig`, `PeerSession`, `bootstrap`).
- First sync orchestration slice is implemented (`sync_step`, `sync_steps`).
- Shelley block deserialization bridge is implemented (`sync_step_decoded`, `decode_shelley_blocks`).
- Next: staged handoff into ledger/storage apply paths and long-running sync loop integration.
