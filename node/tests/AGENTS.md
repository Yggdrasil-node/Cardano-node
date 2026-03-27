# Guidance for node runtime and integration smoke tests.
Keep tests in this directory focused on node orchestration and cross-crate integration behavior.

## Scope
- Runtime bootstrap, sync service, shutdown, and TxSubmission integration tests.
- Managed service behavior and configuration-facing smoke coverage.

##  Rules *Non-Negotiable*
- Tests here MUST stay at the integration boundary and MUST NOT become the primary place for ledger or consensus unit logic.
- Runtime coverage MUST protect startup, shutdown, and protocol-completion paths.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- [Node runtime integration (`cardano-node`)](https://github.com/IntersectMBO/cardano-node/tree/master/cardano-node/)
- [Submit API integration behavior](https://github.com/IntersectMBO/cardano-node/tree/master/cardano-submit-api/)
- [Consensus integration tests](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus-cardano/test/)
- [Consensus diffusion integration](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus-diffusion)

## Current Phase
- Tests in this directory cover runtime bootstrap, sync orchestration, verified service shutdown, TxSubmission integration behavior, and coordinated-storage recovery behavior across the node crate boundary, including first-Shelley activation of pending genesis initial funds and static stake delegations during replay.