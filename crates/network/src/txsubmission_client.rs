//! TxSubmission2 mini-protocol client driver.
//!
//! The TxSubmission2 protocol is pull-based: the *server* requests transaction
//! identifiers and bodies from the *client*.  This driver wraps a
//! [`ProtocolHandle`] and maintains the state machine, providing typed
//! methods to initialise the protocol, receive server requests, and send
//! replies.
//!
//! The client-side waiting states (`StIdle`) are all `waitForever` upstream
//! since the server drives the conversation.  The per-state time limits from
//! `protocol_limits::txsubmission` are therefore all `None` on the client
//! side.  The infrastructure is present for forward-compatibility.
//!
//! Upstream reference:
//! `Ouroboros.Network.Protocol.TxSubmission2.Codec.timeLimitsTxSubmission2`.
//!
//! Reference: `Ouroboros.Network.Protocol.TxSubmission2.Client`.

use std::time::Duration;

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocol_limits::txsubmission as tx_limits;
use crate::protocols::{
    TxIdAndSize, TxSubmissionMessage, TxSubmissionState, TxSubmissionTransitionError,
};
use std::collections::{BTreeSet, VecDeque};
use yggdrasil_ledger::{MultiEraSubmittedTx, Tx, TxId};

// ---------------------------------------------------------------------------
// Server request types
// ---------------------------------------------------------------------------

/// A request from the server to the TxSubmission client.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TxServerRequest {
    /// The server asks for transaction identifiers.
    RequestTxIds {
        /// `true` if the server is blocking (wants non-empty reply or MsgDone).
        blocking: bool,
        /// Number of previously advertised txids to acknowledge.
        ack: u16,
        /// Maximum number of new txids the server wants.
        req: u16,
    },
    /// The server asks for specific transactions by id.
    RequestTxs {
        /// Transaction identifiers to fetch.
        txids: Vec<TxId>,
    },
}

// ---------------------------------------------------------------------------
// Client error
// ---------------------------------------------------------------------------

/// Errors from the TxSubmission client driver.
#[derive(Debug, thiserror::Error)]
pub enum TxSubmissionClientError {
    /// Multiplexer transport error.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),

    /// Connection closed by the remote peer.
    #[error("connection closed")]
    ConnectionClosed,

    /// The server did not respond within the per-state time limit.
    ///
    /// Upstream: `ExceededTimeLimit` from `ProtocolTimeLimits`.
    /// Note: all client-side waiting states in TxSubmission2 are currently
    /// `waitForever` because the server drives the conversation.
    #[error("protocol timeout ({0:?})")]
    Timeout(Duration),

    /// State machine violation.
    #[error("protocol error: {0}")]
    Protocol(#[from] TxSubmissionTransitionError),

    /// CBOR decode failure.
    #[error("CBOR decode error: {0}")]
    Decode(String),

    /// Unexpected message from the server.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),

    /// The server acknowledged more transaction identifiers than are
    /// outstanding in the client FIFO.
    #[error("server acknowledged {ack} txids but only {outstanding} are outstanding")]
    AckedTooManyTxIds { ack: u16, outstanding: u16 },

    /// The server used a blocking txid request while the client still has
    /// outstanding unacknowledged transaction identifiers.
    #[error("blocking txid request with {remaining} outstanding txids")]
    BlockingRequestHasOutstandingTxIds { remaining: u16 },

    /// The server used a non-blocking txid request even though the client has
    /// no outstanding unacknowledged transaction identifiers.
    #[error("non-blocking txid request with no outstanding txids")]
    NonBlockingRequestWithoutOutstandingTxIds,

    /// The server requested a transaction identifier that is not currently
    /// outstanding and requestable.
    #[error("server requested unavailable txid {txid}")]
    RequestedUnavailableTxId { txid: TxId },

    /// The client attempted to advertise the same outstanding transaction id
    /// more than once.
    #[error("duplicate outstanding advertised txid {txid}")]
    DuplicateAdvertisedTxId { txid: TxId },

    /// A blocking txid request must be answered with at least one txid or a
    /// `MsgDone` termination.
    #[error("blocking txid request cannot be answered with an empty txid list")]
    EmptyBlockingReplyTxIds,

    /// The client attempted to reply with more transactions than were
    /// requested.
    #[error("client returned {returned} txs but only {requested} were requested")]
    TooManyTransactionsReturned { returned: usize, requested: usize },

    /// The client attempted to return a typed transaction that was not part of
    /// the current request.
    #[error("client returned unrequested txid {txid}")]
    ReturnedUnrequestedTxId { txid: TxId },
}

// ---------------------------------------------------------------------------
// TxSubmissionClient
// ---------------------------------------------------------------------------

/// A TxSubmission2 client driver maintaining the protocol state machine.
///
/// Usage:
/// 1. Call [`init`] to send `MsgInit`.
/// 2. Call [`recv_request`] to receive the next server request.
/// 3. Call [`reply_tx_ids`] or [`reply_txs`] depending on the request.
/// 4. Repeat from step 2.
/// 5. Call [`done`] from a blocking `StTxIds` state to terminate.
pub struct TxSubmissionClient {
    channel: MessageChannel,
    state: TxSubmissionState,
    outstanding_txids: VecDeque<TxId>,
    requestable_txids: BTreeSet<TxId>,
    requested_txids: Vec<TxId>,
}

impl TxSubmissionClient {
    /// Create a new client driver from a TxSubmission `ProtocolHandle`.
    ///
    /// The protocol starts in `StInit` — client agency.
    pub fn new(handle: ProtocolHandle) -> Self {
        Self {
            channel: MessageChannel::new(handle),
            state: TxSubmissionState::StInit,
            outstanding_txids: VecDeque::new(),
            requestable_txids: BTreeSet::new(),
            requested_txids: Vec::new(),
        }
    }

    /// Returns the current protocol state.
    pub fn state(&self) -> TxSubmissionState {
        self.state
    }

    /// Returns the outstanding transaction-id FIFO maintained for
    /// acknowledgement tracking.
    pub fn outstanding_txids(&self) -> Vec<TxId> {
        self.outstanding_txids.iter().copied().collect()
    }

    // -- helpers ----------------------------------------------------------

    async fn send_msg(&mut self, msg: &TxSubmissionMessage) -> Result<(), TxSubmissionClientError> {
        self.state = self.state.transition(msg)?;
        self.channel
            .send(msg.to_cbor())
            .await
            .map_err(TxSubmissionClientError::Mux)
    }

    /// Receive with an optional per-state time limit.
    async fn recv_msg_timeout(
        &mut self,
        limit: Option<Duration>,
    ) -> Result<TxSubmissionMessage, TxSubmissionClientError> {
        let raw = match limit {
            Some(d) => tokio::time::timeout(d, self.channel.recv())
                .await
                .map_err(|_| TxSubmissionClientError::Timeout(d))?
                .ok_or(TxSubmissionClientError::ConnectionClosed)?,
            None => self
                .channel
                .recv()
                .await
                .ok_or(TxSubmissionClientError::ConnectionClosed)?,
        };
        let msg = TxSubmissionMessage::from_cbor(&raw)
            .map_err(|e| TxSubmissionClientError::Decode(e.to_string()))?;
        self.state = self.state.transition(&msg)?;
        Ok(msg)
    }

    // -- public API -------------------------------------------------------

    /// Send `MsgInit` to initialise the protocol.
    ///
    /// Must be called exactly once, immediately after construction.
    pub async fn init(&mut self) -> Result<(), TxSubmissionClientError> {
        self.send_msg(&TxSubmissionMessage::MsgInit).await
    }

    /// Wait for the next server request.
    ///
    /// The client must be in `StIdle` (server agency).  Uses
    /// `txsubmission::ST_IDLE` time limit (currently `waitForever`).
    /// Returns either `TxServerRequest::RequestTxIds` or
    /// `TxServerRequest::RequestTxs`.
    pub async fn recv_request(&mut self) -> Result<TxServerRequest, TxSubmissionClientError> {
        let msg = self.recv_msg_timeout(tx_limits::ST_IDLE).await?;
        match msg {
            TxSubmissionMessage::MsgRequestTxIds { blocking, ack, req } => {
                self.apply_acknowledgements(ack)?;
                if blocking {
                    if !self.outstanding_txids.is_empty() {
                        return Err(
                            TxSubmissionClientError::BlockingRequestHasOutstandingTxIds {
                                remaining: self.outstanding_txids.len() as u16,
                            },
                        );
                    }
                } else if self.outstanding_txids.is_empty() {
                    return Err(TxSubmissionClientError::NonBlockingRequestWithoutOutstandingTxIds);
                }
                Ok(TxServerRequest::RequestTxIds { blocking, ack, req })
            }
            TxSubmissionMessage::MsgRequestTxs { txids } => {
                let mut seen = BTreeSet::new();
                for txid in &txids {
                    if !seen.insert(*txid) || !self.requestable_txids.remove(txid) {
                        return Err(TxSubmissionClientError::RequestedUnavailableTxId {
                            txid: *txid,
                        });
                    }
                }
                self.requested_txids = txids.clone();
                Ok(TxServerRequest::RequestTxs { txids })
            }
            other => Err(TxSubmissionClientError::UnexpectedMessage(
                other.tag_name().to_string(),
            )),
        }
    }

    /// Reply with transaction identifiers.
    ///
    /// The client must be in `StTxIds`.
    pub async fn reply_tx_ids(
        &mut self,
        txids: Vec<TxIdAndSize>,
    ) -> Result<(), TxSubmissionClientError> {
        if matches!(self.state, TxSubmissionState::StTxIds { blocking: true }) && txids.is_empty() {
            return Err(TxSubmissionClientError::EmptyBlockingReplyTxIds);
        }
        for item in &txids {
            if self.requestable_txids.contains(&item.txid)
                || self.outstanding_txids.iter().any(|txid| *txid == item.txid)
            {
                return Err(TxSubmissionClientError::DuplicateAdvertisedTxId { txid: item.txid });
            }
        }
        let advertised_txids: Vec<_> = txids.iter().map(|item| item.txid).collect();
        self.send_msg(&TxSubmissionMessage::MsgReplyTxIds { txids })
            .await?;
        for txid in advertised_txids {
            self.outstanding_txids.push_back(txid);
            self.requestable_txids.insert(txid);
        }
        Ok(())
    }

    /// Reply with transaction bodies.
    ///
    /// The client must be in `StTxs`.
    pub async fn reply_txs(&mut self, txs: Vec<Vec<u8>>) -> Result<(), TxSubmissionClientError> {
        self.reply_txs_internal(None, txs).await
    }

    /// Reply with typed ledger transactions.
    ///
    /// The wire protocol carries only serialized transaction bodies, so this
    /// helper strips the canonical `Tx` wrapper to preserve a typed client API.
    pub async fn reply_txs_typed(&mut self, txs: Vec<Tx>) -> Result<(), TxSubmissionClientError> {
        let txids = txs.iter().map(|tx| tx.id).collect();
        let txs = txs.into_iter().map(|tx| tx.body).collect();
        self.reply_txs_internal(Some(txids), txs).await
    }

    /// Reply with typed multi-era submitted transactions.
    ///
    /// Each transaction is serialized using the exact or reconstructed CBOR
    /// bytes carried by the ledger submission wrapper.
    pub async fn reply_txs_multi_era(
        &mut self,
        txs: Vec<MultiEraSubmittedTx>,
    ) -> Result<(), TxSubmissionClientError> {
        let txids = txs.iter().map(MultiEraSubmittedTx::tx_id).collect();
        let txs = txs.into_iter().map(|tx| tx.raw_cbor()).collect();
        self.reply_txs_internal(Some(txids), txs).await
    }

    /// Send `MsgDone` to terminate the protocol.
    ///
    /// The client must be in `StTxIds { blocking: true }`.
    pub async fn send_done(&mut self) -> Result<(), TxSubmissionClientError> {
        self.send_msg(&TxSubmissionMessage::MsgDone).await
    }

    /// Send `MsgDone` to terminate the protocol, consuming the client.
    ///
    /// The client must be in `StTxIds { blocking: true }`.
    pub async fn done(mut self) -> Result<(), TxSubmissionClientError> {
        self.send_done().await
    }

    fn apply_acknowledgements(&mut self, ack: u16) -> Result<(), TxSubmissionClientError> {
        if usize::from(ack) > self.outstanding_txids.len() {
            return Err(TxSubmissionClientError::AckedTooManyTxIds {
                ack,
                outstanding: self.outstanding_txids.len() as u16,
            });
        }

        for _ in 0..ack {
            if let Some(txid) = self.outstanding_txids.pop_front() {
                self.requestable_txids.remove(&txid);
            }
        }

        Ok(())
    }

    async fn reply_txs_internal(
        &mut self,
        returned_txids: Option<Vec<TxId>>,
        txs: Vec<Vec<u8>>,
    ) -> Result<(), TxSubmissionClientError> {
        let requested = self.requested_txids.clone();
        if txs.len() > requested.len() {
            return Err(TxSubmissionClientError::TooManyTransactionsReturned {
                returned: txs.len(),
                requested: requested.len(),
            });
        }

        if let Some(returned_txids) = returned_txids {
            let requested_set: BTreeSet<_> = requested.iter().copied().collect();
            let mut seen = BTreeSet::new();
            for txid in returned_txids {
                if !seen.insert(txid) || !requested_set.contains(&txid) {
                    return Err(TxSubmissionClientError::ReturnedUnrequestedTxId { txid });
                }
            }
        }

        self.send_msg(&TxSubmissionMessage::MsgReplyTxs { txs })
            .await?;
        self.requested_txids.clear();
        Ok(())
    }
}
