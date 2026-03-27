# Guidance for mempool queue, snapshot, and admission implementation modules.

This directory owns queue policy and typed mempool views, not ledger validation semantics.

## Scope
- Queue ordering, snapshot traversal, duplicate detection, and eviction helpers.
- Shared and non-shared mempool reader implementations.

##  Rules *Non-Negotiable*
- Queue semantics MUST remain explicit and locally testable from this directory.
- Networking protocol concerns MUST not leak into mempool internals here.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- Mempool API (`API.hs`): <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Mempool/API.hs>
- Mempool TxSeq (`TxSeq.hs`): <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Mempool/TxSeq.hs>
- Mempool Capacity (`Capacity.hs`): <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Mempool/Capacity.hs>
- Mempool implementation (`Impl/`): <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Mempool/Impl>
- Submit API integration reference: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-submit-api/>

## Current Phase
- Preserve the separation between fee ordering and TxSubmission snapshot ordering.