//! Mempool integration helpers for the TxSubmission2 server side.
//!
//! Mirrors upstream `Ouroboros.Network.TxSubmission.Inbound.Server` +
//! `Ouroboros.Consensus.Mempool.Update` glue: a TxSubmission2 inbound
//! server hands txs to the mempool one at a time (or as a batch),
//! observing per-tx outcomes for tracing and back-pressure.
//!
//! Two parallel APIs are exposed:
//! - `add_tx_to_mempool` / `add_txs_to_mempool` — direct, take a `&LedgerState`
//!   and `&mut Mempool`; for unit tests and callers holding exclusive locks.
//! - `add_tx_to_shared_mempool` / `add_txs_to_shared_mempool` — through
//!   `SharedMempool` + `SharedTxState` for production runtime use, where
//!   the mempool is shared across sync, governor, and TxSubmission tasks.
//!
//! The `*_with_eviction` variants additionally re-validate after add and
//! evict any transactions that fail revalidation against the post-add
//! ledger state — used by R157+ slip-batch eviction handling.
//!
//! Extracted from `runtime.rs` in R271d.

use yggdrasil_consensus::mempool::{Mempool, MempoolEntry, MempoolError, SharedMempool};
use yggdrasil_ledger::{
    LedgerError, LedgerState, MultiEraSubmittedTx, SlotNo, TxId, plutus_validation::PlutusEvaluator,
};

/// Result of attempting to add a single transaction to the mempool.
///
/// This mirrors the upstream `MempoolAddTxResult` split between accepted and
/// rejected transactions while keeping queue-level failures separate.
#[derive(Debug, Eq, PartialEq)]
pub enum MempoolAddTxResult {
    /// The transaction was validated and added to the mempool.
    MempoolTxAdded(TxId),
    /// The transaction was rejected by ledger validation and not added.
    MempoolTxRejected(TxId, LedgerError),
}

/// Queue-level failures encountered while adding a transaction to the mempool.
#[derive(Debug, thiserror::Error)]
pub enum MempoolAddTxError {
    /// Underlying mempool capacity, duplicate, or TTL error.
    #[error("mempool admission error: {0}")]
    Mempool(#[from] MempoolError),
}

fn admitted_entry(tx: MultiEraSubmittedTx) -> MempoolEntry {
    let fee = tx.fee();
    let ttl = tx.expires_at().unwrap_or(SlotNo(u64::MAX));
    MempoolEntry::from_multi_era_submitted_tx(tx, fee, ttl)
}

fn add_tx_with<F>(
    ledger: &mut LedgerState,
    tx: MultiEraSubmittedTx,
    current_slot: SlotNo,
    evaluator: Option<&dyn PlutusEvaluator>,
    mut insert_entry: F,
) -> Result<MempoolAddTxResult, MempoolAddTxError>
where
    F: FnMut(
        MempoolEntry,
        Option<&yggdrasil_ledger::ProtocolParameters>,
    ) -> Result<(), MempoolError>,
{
    let tx_id = tx.tx_id();
    let mut staged_ledger = ledger.clone();
    match staged_ledger.apply_submitted_tx(&tx, current_slot, evaluator) {
        Ok(()) => {
            insert_entry(admitted_entry(tx), staged_ledger.protocol_params())?;
            *ledger = staged_ledger;
            Ok(MempoolAddTxResult::MempoolTxAdded(tx_id))
        }
        Err(err) => Ok(MempoolAddTxResult::MempoolTxRejected(tx_id, err)),
    }
}

/// Validate and add a single transaction to the mempool.
///
/// The transaction is first applied to a staged clone of the caller-provided
/// ledger state. If ledger validation fails, the ledger and mempool remain
/// unchanged and the result is `MempoolTxRejected`. If validation succeeds, the
/// transaction is inserted into the mempool and the staged ledger state is
/// committed.
pub fn add_tx_to_mempool(
    ledger: &mut LedgerState,
    mempool: &mut Mempool,
    tx: MultiEraSubmittedTx,
    current_slot: SlotNo,
    evaluator: Option<&dyn PlutusEvaluator>,
) -> Result<MempoolAddTxResult, MempoolAddTxError> {
    add_tx_with(
        ledger,
        tx,
        current_slot,
        evaluator,
        |entry, protocol_params| mempool.insert_checked(entry, current_slot, protocol_params),
    )
}

/// Validate and add a single transaction to a shared mempool.
///
/// This is the shared-handle variant of [`add_tx_to_mempool`]. Accepted
/// transactions update the caller's ledger state only after the shared mempool
/// insert succeeds.
pub fn add_tx_to_shared_mempool(
    ledger: &mut LedgerState,
    mempool: &SharedMempool,
    tx: MultiEraSubmittedTx,
    current_slot: SlotNo,
    evaluator: Option<&dyn PlutusEvaluator>,
) -> Result<MempoolAddTxResult, MempoolAddTxError> {
    add_tx_with(
        ledger,
        tx,
        current_slot,
        evaluator,
        |entry, protocol_params| mempool.insert_checked(entry, current_slot, protocol_params),
    )
}

/// Validate and add a sequence of transactions to the mempool in order.
///
/// This mirrors the upstream `addTxs` semantics: each transaction is checked
/// against the ledger state produced by all previously accepted transactions in
/// the same batch. Rejected transactions do not advance the staged ledger
/// state. Queue-level failures stop the batch and return an error.
pub fn add_txs_to_mempool<I>(
    ledger: &mut LedgerState,
    mempool: &mut Mempool,
    txs: I,
    current_slot: SlotNo,
    evaluator: Option<&dyn PlutusEvaluator>,
) -> Result<Vec<MempoolAddTxResult>, MempoolAddTxError>
where
    I: IntoIterator<Item = MultiEraSubmittedTx>,
{
    txs.into_iter()
        .map(|tx| add_tx_to_mempool(ledger, mempool, tx, current_slot, evaluator))
        .collect()
}

/// Validate and add a sequence of transactions to a shared mempool in order.
///
/// Accepted transactions update the caller's ledger state one by one so later
/// transactions in the batch can depend on earlier accepted outputs.
pub fn add_txs_to_shared_mempool<I>(
    ledger: &mut LedgerState,
    mempool: &SharedMempool,
    txs: I,
    current_slot: SlotNo,
    evaluator: Option<&dyn PlutusEvaluator>,
) -> Result<Vec<MempoolAddTxResult>, MempoolAddTxError>
where
    I: IntoIterator<Item = MultiEraSubmittedTx>,
{
    txs.into_iter()
        .map(|tx| add_tx_to_shared_mempool(ledger, mempool, tx, current_slot, evaluator))
        .collect()
}

/// Outcome of an eviction-aware inbound submission attempt: the per-tx
/// admission result plus the list of `TxId`s that were evicted from the
/// mempool's lowest-fee tail to make room (possibly empty).
///
/// Mirrors upstream
/// `Ouroboros.Consensus.Mempool.Impl.Update.makeRoomForTransaction` —
/// each accepted transaction returns the set of displaced TxIds so the
/// caller can prune downstream peer-relay state (e.g. trace
/// observability, future cross-peer dedup invalidation).
#[derive(Debug)]
pub struct MempoolAddTxOutcome {
    /// The admission result (added or rejected).
    pub result: MempoolAddTxResult,
    /// `TxId`s of mempool entries that were evicted to make room for the
    /// admitted transaction. Empty when the new transaction fit without
    /// displacement OR when the result is `MempoolTxRejected` (in which
    /// case nothing was evicted).
    pub evicted: Vec<TxId>,
}

/// Validate and add a single transaction to a shared mempool, falling
/// back to lowest-fee eviction on capacity overflow.
///
/// Composes ledger validation (`apply_submitted_tx`) with
/// `SharedMempool::insert_checked_with_eviction` so the same upstream-
/// aligned `Ouroboros.Consensus.Mempool.Impl.Update.makeRoomForTransaction`
/// semantics that protect cumulative pool revenue also apply at the
/// inbound submission boundary (NtN TxSubmission, NtC LocalTxSubmission).
/// Returns the displaced `TxId`s so the caller can attach them to a
/// trace event for operator visibility.
pub fn add_tx_to_shared_mempool_with_eviction(
    ledger: &mut LedgerState,
    mempool: &SharedMempool,
    tx: MultiEraSubmittedTx,
    current_slot: SlotNo,
    evaluator: Option<&dyn PlutusEvaluator>,
) -> Result<MempoolAddTxOutcome, MempoolAddTxError> {
    let mut evicted: Vec<TxId> = Vec::new();
    let evicted_ref = &mut evicted;
    let result = add_tx_with(
        ledger,
        tx,
        current_slot,
        evaluator,
        |entry, protocol_params| match mempool.insert_checked_with_eviction(
            entry,
            current_slot,
            protocol_params,
        ) {
            Ok(displaced) => {
                *evicted_ref = displaced;
                Ok(())
            }
            Err(e) => Err(e),
        },
    )?;
    Ok(MempoolAddTxOutcome { result, evicted })
}

/// Sequence variant of [`add_tx_to_shared_mempool_with_eviction`]:
/// validates each transaction against the ledger state produced by all
/// previously accepted transactions in the same batch, and accumulates
/// evicted TxIds across the whole batch.
pub fn add_txs_to_shared_mempool_with_eviction<I>(
    ledger: &mut LedgerState,
    mempool: &SharedMempool,
    txs: I,
    current_slot: SlotNo,
    evaluator: Option<&dyn PlutusEvaluator>,
) -> Result<Vec<MempoolAddTxOutcome>, MempoolAddTxError>
where
    I: IntoIterator<Item = MultiEraSubmittedTx>,
{
    txs.into_iter()
        .map(|tx| {
            add_tx_to_shared_mempool_with_eviction(ledger, mempool, tx, current_slot, evaluator)
        })
        .collect()
}
