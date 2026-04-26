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
- `insert_with_eviction(entry)` mirrors upstream `Ouroboros.Consensus.Mempool.Impl.Update.makeRoomForTransaction`: when capacity is exceeded, walks the lowest-fee tail (entries are already fee-descending sorted) and tentatively evicts entries whose fee is **strictly less** than the incoming fee until enough bytes are freed. The eviction commits only when the cumulative evicted fee is also strictly less than the incoming fee — otherwise the helper returns `MempoolError::EvictionNotWorthwhile { incoming_fee, evicted_fee }` so the network is never displaced into a worse cumulative-fee state. When the incoming entry exceeds the mempool's total capacity (no eviction can ever fit it) the helper returns the new `MempoolError::EvictionInsufficientSpace { incoming, limit, freeable }` rather than the misleading `CapacityExceeded`. Returns `Vec<TxId>` of evicted entries on success so the caller can prune downstream peer-relay state. Duplicate-tx and conflicting-input checks fire BEFORE eviction is considered (same as `insert`), so a replay can never displace unrelated low-fee entries. `SharedMempool::insert_with_eviction` proxies the lock + `change_notify.notify_waiters()` notification. 7 new regression tests in `queue::tests` cover the fast path under capacity, the happy displacement path, the not-worthwhile rejection, the head-protection guard against equal-or-higher-fee entries, the incoming-too-large-for-total-capacity path, the duplicate-short-circuit, and the SharedMempool wrapper.
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
