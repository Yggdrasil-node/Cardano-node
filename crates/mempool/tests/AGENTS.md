# Guidance for mempool admission, ordering, and eviction tests.

Use this directory to pin queue semantics and mempool boundary behavior.

## Scope
- Admission and duplicate rejection.
- Ordering, TTL expiry, snapshot traversal, and block-confirmation eviction.

##  Rules *Non-Negotiable*
- Tests here MUST assert queue behavior directly rather than relying on node integration side effects.
- Snapshot and shared-reader coverage MUST protect monotonic cursor semantics.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research references and add or update links as needed*
- Mempool module root: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Mempool>
- Mempool tests: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/test>
- Submit API integration reference: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-submit-api/>

## Current Phase
- Tests in this directory pin duplicate rejection, fee ordering, TTL expiry, shared snapshots, and confirmation-driven eviction.