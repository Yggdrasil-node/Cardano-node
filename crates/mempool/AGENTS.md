# Guidance for transaction admission, ordering, and eviction work
Focus on deterministic transaction intake and on keeping ledger validation and queue policy separate.

## Scope
- Transaction admission, prioritization, eviction, and rollback-aware removal.
- Boundaries between queue policy, ledger validation, and network submission.

##  Rules *Non-Negotiable*
- Admission checks and prioritization logic MUST remain explicit and testable.
- Mempool ordering MUST NOT be coupled to networking concerns.
- Ledger validation MUST be treated as an input contract, not a hidden side effect.
- Rollback and block-application eviction MUST be accounted for from the start.
- Public mempool types and functions MUST have Rustdocs when queue semantics, ordering rules, or eviction behavior matter to callers.
- Naming MUST stay close to official node and consensus mempool terminology.
- Transaction flow and admission policy MUST be explained with reference to the official node and upstream mempool-adjacent sources such as Ouroboros consensus and `cardano-submit-api`.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- Consensus Mempool module (`API.hs`, `TxSeq.hs`, `Capacity.hs`, `Init.hs`, `Update.hs`, `Query.hs`): <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Mempool/>
- Mempool implementation internals: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Mempool/Impl/>
- Consensus repository root (for broader context): <https://github.com/IntersectMBO/ouroboros-consensus/>
- Transaction submission API: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-submit-api/>
- Node integration reference: <https://github.com/IntersectMBO/cardano-node/>

## Current Phase
- Mempool uses real ledger `TxId` for transaction identification, stores the transaction era plus both body CBOR and full submitted-transaction CBOR, and can convert entries to/from ledger `MultiEraSubmittedTx` for relay-facing integration.
- Fee-descending ordering is implemented with duplicate detection and capacity enforcement.
- TxSubmission-facing snapshot support is now exposed via upstream-aligned `TxSubmissionMempoolReader` and `MempoolSnapshot` terminology, with a monotonic `MempoolIdx` cursor kept separate from fee ordering. `SharedMempool` and `SharedTxSubmissionMempoolReader` provide concurrent snapshot access for long-lived TxSubmission services without coupling networking to queue internals.
- `remove_by_id`, `contains`, `len`, `is_empty`, `size_bytes`, `iter`, and `remove_confirmed` are implemented.
- Block-application eviction via `remove_confirmed` enables post-sync snipping of confirmed transactions.
- TTL-aware admission via `insert_checked(entry, current_slot, protocol_params)` rejects transactions whose TTL has expired and, when protocol parameters are supplied, re-validates transaction body size and minimum fee thresholds (`maxTxSize`, `minFeeA`, `minFeeB`) at mempool admission time.
- For decodable Alonzo/Babbage/Conway submitted transactions, `insert_checked` now also aggregates redeemer `ExUnits`, enforces `maxTxExUnits`, and applies script-fee-aware minimum-fee checks before admission.
- Periodic TTL expiry purge via `purge_expired(current_slot)` removes all stale entries.
- Node-side `evict_confirmed_from_mempool` in `node/src/sync.rs` wires mempool eviction into the sync pipeline.
- Fee ordering remains a queue policy concern; TxSubmission snapshot traversal uses insertion-order indices so networking does not depend on fee-priority layout.
- Shared mempool intake now carries protocol-parameter-aware admission checks through node runtime call paths; next work is broader network submission sourcing and any additional policy harmonization with upstream mempool revalidation behavior.
- `remove_conflicting_inputs(consumed: &[ShelleyTxIn])` evicts any mempool entry whose inputs overlap with the given consumed-input set. Wired into the sync pipeline alongside `remove_confirmed()` so that when a block application consumes inputs needed by a mempool transaction (double-spend conflict), that transaction is removed promptly. Reference: `Ouroboros.Consensus.Mempool.Impl.Update` — `syncWithLedger`.
