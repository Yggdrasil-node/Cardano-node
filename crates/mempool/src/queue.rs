use yggdrasil_ledger::{Era, LedgerError, MultiEraSubmittedTx, SlotNo, TxId};

/// A mempool entry carrying a transaction identifier and its fee for ordering.
///
/// The `tx_id` is the Blake2b-256 hash of the CBOR-encoded transaction body,
/// matching the upstream Cardano `TxId` representation.
///
/// Reference: `Cardano.Ledger.TxIn` — `TxId`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MempoolEntry {
    /// The era needed to decode the submitted transaction relay payload.
    pub era: Era,
    /// The transaction identifier (Blake2b-256 of CBOR body).
    pub tx_id: TxId,
    /// The transaction fee in lovelace, used for ordering.
    pub fee: u64,
    /// The raw CBOR-encoded transaction body bytes.
    pub body: Vec<u8>,
    /// The raw CBOR-encoded submitted transaction bytes used for relay.
    pub raw_tx: Vec<u8>,
    /// Size of the transaction in bytes (for capacity tracking).
    pub size_bytes: usize,
    /// Time-to-live slot — the transaction is invalid after this slot.
    /// Matches the Shelley `ttl` field semantics: valid while `current_slot <= ttl`.
    pub ttl: SlotNo,
}

/// Error type for converting mempool entries into typed submitted transactions.
#[derive(Debug, thiserror::Error)]
pub enum MempoolRelayError {
    /// The stored submitted transaction bytes could not be decoded for the
    /// entry's era.
    #[error("failed to decode submitted transaction for era {era:?}: {source}")]
    Decode {
        /// Era used to decode the submitted transaction payload.
        era: Era,
        /// Underlying ledger decode failure.
        source: LedgerError,
    },
    /// The entry's stored body bytes do not match the decoded transaction
    /// body.
    #[error("mempool entry body bytes do not match decoded submitted transaction body")]
    BodyMismatch,
    /// The entry's stored `TxId` does not match the decoded transaction body.
    #[error("mempool entry txid {expected} does not match decoded txid {actual}")]
    TxIdMismatch {
        /// TxId stored in the mempool entry.
        expected: TxId,
        /// TxId recomputed from the decoded submitted transaction.
        actual: TxId,
    },
}

impl MempoolEntry {
    /// Build a mempool entry from a typed multi-era submitted transaction.
    pub fn from_multi_era_submitted_tx(
        tx: MultiEraSubmittedTx,
        fee: u64,
        ttl: SlotNo,
    ) -> Self {
        let era = tx.era();
        let tx_id = tx.tx_id();
        let body = tx.body_cbor();
        let raw_tx = tx.raw_cbor();
        let size_bytes = raw_tx.len();
        Self {
            era,
            tx_id,
            fee,
            body,
            raw_tx,
            size_bytes,
            ttl,
        }
    }

    /// Decode the relay payload into a typed multi-era submitted transaction.
    pub fn to_multi_era_submitted_tx(&self) -> Result<MultiEraSubmittedTx, MempoolRelayError> {
        let tx = MultiEraSubmittedTx::from_cbor_bytes_for_era(self.era, &self.raw_tx).map_err(
            |source| MempoolRelayError::Decode {
                era: self.era,
                source,
            },
        )?;
        if tx.body_cbor() != self.body {
            return Err(MempoolRelayError::BodyMismatch);
        }
        let actual = tx.tx_id();
        if actual != self.tx_id {
            return Err(MempoolRelayError::TxIdMismatch {
                expected: self.tx_id,
                actual,
            });
        }
        Ok(tx)
    }
}

/// Error type for mempool operations.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MempoolError {
    /// The transaction is already in the mempool.
    #[error("duplicate transaction: {0:?}")]
    Duplicate(TxId),
    /// Adding this transaction would exceed the mempool capacity.
    #[error("mempool capacity exceeded: {current} + {incoming} > {limit}")]
    CapacityExceeded {
        /// Current total bytes in the mempool.
        current: usize,
        /// Size of the incoming transaction.
        incoming: usize,
        /// Maximum capacity in bytes.
        limit: usize,
    },
    /// The transaction has already expired at the given slot.
    #[error("transaction TTL expired: ttl {ttl:?} < current slot {current_slot:?}")]
    TtlExpired {
        /// The transaction's TTL slot.
        ttl: SlotNo,
        /// The current slot at admission time.
        current_slot: SlotNo,
    },
}

/// A fee-ordered mempool with capacity tracking and duplicate detection.
///
/// Entries are ordered by descending fee so that `pop_best` always returns
/// the highest-fee transaction. Size tracking prevents unbounded growth.
///
/// Reference: `Ouroboros.Consensus.Mempool.API` — `Mempool`.
#[derive(Clone, Debug, Default)]
pub struct Mempool {
    entries: Vec<MempoolEntry>,
    /// Maximum aggregate size in bytes (0 = unlimited).
    max_bytes: usize,
    /// Current total size in bytes of all entries.
    current_bytes: usize,
}

impl Mempool {
    /// Create a new mempool with the given maximum capacity in bytes.
    ///
    /// A `max_bytes` of 0 means no capacity limit.
    pub fn with_capacity(max_bytes: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_bytes,
            current_bytes: 0,
        }
    }

    /// Insert an entry if it does not already exist and fits within capacity.
    ///
    /// Keeps the queue ordered by descending fee.
    pub fn insert(&mut self, entry: MempoolEntry) -> Result<(), MempoolError> {
        if self.entries.iter().any(|e| e.tx_id == entry.tx_id) {
            return Err(MempoolError::Duplicate(entry.tx_id));
        }
        if self.max_bytes > 0 && self.current_bytes + entry.size_bytes > self.max_bytes {
            return Err(MempoolError::CapacityExceeded {
                current: self.current_bytes,
                incoming: entry.size_bytes,
                limit: self.max_bytes,
            });
        }
        self.current_bytes += entry.size_bytes;
        self.entries.push(entry);
        self.entries.sort_by(|left, right| right.fee.cmp(&left.fee));
        Ok(())
    }

    /// Remove and return the highest-fee entry, if any.
    pub fn pop_best(&mut self) -> Option<MempoolEntry> {
        if self.entries.is_empty() {
            None
        } else {
            let entry = self.entries.remove(0);
            self.current_bytes -= entry.size_bytes;
            Some(entry)
        }
    }

    /// Remove a transaction by its identifier. Returns `true` if found.
    pub fn remove_by_id(&mut self, tx_id: &TxId) -> bool {
        if let Some(pos) = self.entries.iter().position(|e| &e.tx_id == tx_id) {
            let entry = self.entries.remove(pos);
            self.current_bytes -= entry.size_bytes;
            true
        } else {
            false
        }
    }

    /// Check whether a transaction with the given id exists in the mempool.
    pub fn contains(&self, tx_id: &TxId) -> bool {
        self.entries.iter().any(|e| &e.tx_id == tx_id)
    }

    /// Number of transactions currently in the mempool.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the mempool is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Current aggregate size of all entries in bytes.
    pub fn size_bytes(&self) -> usize {
        self.current_bytes
    }

    /// Iterator over entries in fee-descending order.
    pub fn iter(&self) -> impl Iterator<Item = &MempoolEntry> {
        self.entries.iter()
    }

    /// Remove all transactions whose identifiers appear in the given block's
    /// transaction list, as they have been confirmed on-chain.
    ///
    /// Returns the number of entries removed.
    pub fn remove_confirmed(&mut self, confirmed_tx_ids: &[TxId]) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| {
            if confirmed_tx_ids.contains(&e.tx_id) {
                self.current_bytes -= e.size_bytes;
                false
            } else {
                true
            }
        });
        before - self.entries.len()
    }

    /// Insert an entry with TTL validation against the current slot.
    ///
    /// The transaction is rejected if its TTL has already passed
    /// (`current_slot > entry.ttl`). Otherwise it proceeds through the
    /// normal `insert` checks (duplicate detection, capacity).
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — TTL check.
    pub fn insert_checked(
        &mut self,
        entry: MempoolEntry,
        current_slot: SlotNo,
    ) -> Result<(), MempoolError> {
        if current_slot > entry.ttl {
            return Err(MempoolError::TtlExpired {
                ttl: entry.ttl,
                current_slot,
            });
        }
        self.insert(entry)
    }

    /// Remove all transactions whose TTL has passed at the given slot.
    ///
    /// Returns the number of entries removed.
    ///
    /// Reference: `Ouroboros.Consensus.Mempool.Impl.Common` — re-validation.
    pub fn purge_expired(&mut self, current_slot: SlotNo) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| {
            if current_slot > e.ttl {
                self.current_bytes -= e.size_bytes;
                false
            } else {
                true
            }
        });
        before - self.entries.len()
    }
}
