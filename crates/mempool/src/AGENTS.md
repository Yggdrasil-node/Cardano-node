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
- Cross-peer TxId deduplication is implemented in `tx_state.rs`: `TxState` / `SharedTxState` (`Arc<RwLock<TxState>>`) tracks globally-known TxIds (bounded ring, default 16 384), per-peer unacknowledged and in-flight sets, and a `global_in_flight` set. `filter_advertised()` partitions advertised TxIds into `FilterOutcome { to_fetch, already_known }`. `mark_in_flight()` / `mark_received()` / `mark_not_found()` track fetch lifecycle. `unregister_peer()` cancels in-flight and cleans per-peer state. `mark_confirmed()` removes block-confirmed TxIds from all peer sets. Upstream reference: `Ouroboros.Network.TxSubmission.Inbound` shared state (network-level, not mempool-level). Wired into `run_txsubmission_server` in `node/src/server.rs`.